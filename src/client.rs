//! The donor-portal reads, made with a profile's stored session (docs/api.md).
//!
//! - Every read is a `GET` of the site's own JSON under
//!   `/api/invita/mobileuser/resource`, the same requests its pages make.
//! - A request carries the session's `Cookie` header, the browser's user
//!   agent captured with it, and `Accept: application/json`.
//! - It follows no redirect and is bounded in time: 30 s for the whole
//!   answer, 10 s to connect.
//! - One request at a time, at least [`PACE`] after the last answer.
//! - Nothing writes to OneBlood. `oneblood api` sends only `GET`s of the
//!   site's read paths, checked by [`api_path`] before one is sent.
//! - [`judge`] turns an answer into data or an error: a 401, a redirect or
//!   the site's "not signed in" answer is a refused session (exit 3); a 403
//!   page, a 429, a 5xx and an error answer are exit 5.
//! - A `Set-Cookie` on an answer updates the client's jar; the caller writes
//!   the updated session back once the command has read what it needed
//!   ([`Client::updated_cookie_header`]).

use std::cell::{Cell, RefCell};
use std::io::Read as _;
use std::time::{Duration, Instant};

use pk_cli_config::profile::ProfileName;
use pk_cli_core::{CliError, Reason};
use pk_cli_secrets::Secret;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE, RETRY_AFTER, SET_COOKIE, USER_AGENT};
use serde_json::Value;

use crate::cli::BIN;
use crate::session::Session;

/// The site's origin.
pub const BASE: &str = "https://www.oneblood.org";
/// Points the reads at a test's loopback server instead of [`BASE`]
/// ([`base`]).
pub const BASE_URL_ENV: &str = "ONEBLOOD_BASE_URL";
/// The least time between one answer and the next request.
pub const PACE: Duration = Duration::from_millis(750);
const TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of an answer read.
const MAX_JSON: u64 = 16 * 1024 * 1024;

/// The donor's own account: name, donor id, blood type.
pub const USER: &str = "/api/invita/mobileuser/resource";
/// The donations, past and booked.
pub const DONATIONS: &str = "/api/invita/mobileuser/resource/?donations=1";

/// One reading's history: `/api/invita/mobileuser/resource/?tests=<id>`.
pub fn tests_path(test_id: u32) -> String {
    format!("{USER}/?tests={test_id}")
}

/// The origin this process reads: [`BASE`], unless [`BASE_URL_ENV`] names a
/// test's server.
pub fn base() -> Result<String, CliError> {
    base_from(
        std::env::var(BASE_URL_ENV).ok().as_deref(),
        crate::session::is_test_store(),
    )
}

/// The check behind [`base`], on the values passed in. The override is for
/// a test's server: a plain `http` origin on a loopback address, and only
/// over a test's own session store (`session::is_test_store`), so no
/// session the owner stored is sent to it. An ambient variable sending a
/// real session elsewhere would be a way to steal it.
pub fn base_from(value: Option<&str>, test_store: bool) -> Result<String, CliError> {
    let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(BASE.to_string());
    };
    let refuse = |why: &str| {
        CliError::Usage(format!(
            "{BASE_URL_ENV} {why}; it exists only to point tests at a server on this machine"
        ))
    };
    if !test_store {
        return Err(refuse(&format!(
            "needs a test's own session store, so no session the owner stored is sent to it: {} naming a \
             `piekstra.oneblood.<suffix>` service, and XDG_CONFIG_HOME set away from ~/.config",
            crate::session::KEYCHAIN_SERVICE_ENV
        )));
    }
    let url = reqwest::Url::parse(value).map_err(|_| refuse("is not a URL"))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    let plain = url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if !(loopback && plain) {
        return Err(refuse(
            "must be an http origin on a loopback address, like http://127.0.0.1:8080",
        ));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// A session's cookies, in the order the header gave them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jar {
    cookies: Vec<(String, String)>,
    changed: bool,
}

impl Jar {
    pub fn parse(header: &str) -> Jar {
        let cookies = header
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                (!name.trim().is_empty()).then(|| (name.trim().to_string(), value.trim().to_string()))
            })
            .collect();
        Jar {
            cookies,
            changed: false,
        }
    }

    pub fn header(&self) -> String {
        self.cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// One `Set-Cookie` value: a new value replaces the cookie's, a new
    /// cookie is added, and an empty value or `Max-Age` of zero or less
    /// removes it. The other attributes (domain, path, expiry) are the
    /// browser's business; the site's are all on its own hosts.
    pub fn apply(&mut self, set_cookie: &str) {
        let mut parts = set_cookie.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
            return;
        };
        let (name, value) = (name.trim(), value.trim());
        if name.is_empty() {
            return;
        }
        let expired = parts.any(|a| {
            a.trim().split_once('=').is_some_and(|(k, v)| {
                k.trim().eq_ignore_ascii_case("max-age") && v.trim().parse::<i64>().is_ok_and(|n| n <= 0)
            })
        });
        let at = self.cookies.iter().position(|(n, _)| n == name);
        match (at, value.is_empty() || expired) {
            (Some(i), true) => {
                self.cookies.remove(i);
                self.changed = true;
            }
            (Some(i), false) if self.cookies[i].1 != value => {
                self.cookies[i].1 = value.to_string();
                self.changed = true;
            }
            (None, false) => {
                self.cookies.push((name.to_string(), value.to_string()));
                self.changed = true;
            }
            _ => {}
        }
    }

    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// What a read sends and to whom: the session's cookies and user agent.
pub struct Client {
    http: reqwest::blocking::Client,
    base: String,
    jar: RefCell<Jar>,
    pace: Duration,
    last: Cell<Option<Instant>>,
    profile: ProfileName,
}

impl Client {
    /// A client for `profile`'s `session` against `base` ([`BASE`], or a
    /// test's loopback server), `pace` apart.
    pub fn new(session: &Session, profile: &ProfileName, base: &str, pace: Duration) -> Result<Client, CliError> {
        let agent = session
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("{BIN}/{}", env!("CARGO_PKG_VERSION")));
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&agent)
                .map_err(|_| CliError::Other("the stored user agent is not a valid HTTP header value".into()))?,
        );
        // A plain client: no cookie store (the jar is ours), no redirects.
        let http = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| CliError::Other(format!("building the HTTP client: {}", e.without_url())))?;
        Ok(Client {
            http,
            base: base.trim_end_matches('/').to_string(),
            jar: RefCell::new(Jar::parse(session.cookie_header.expose())),
            pace,
            last: Cell::new(None),
            profile: profile.clone(),
        })
    }

    /// `GET path`: the answer's `data`, judged by [`judge`].
    pub fn data(&self, path: &str) -> Result<Value, CliError> {
        let answer = self.get(path)?;
        judge(&answer, &self.profile)
    }

    /// `GET path` for `oneblood api`: the whole JSON answer as it came, after
    /// the refusals every read gets.
    pub fn api(&self, path: &str) -> Result<Value, CliError> {
        let answer = self.get(path)?;
        refuse(&answer, &self.profile)?;
        let what = &answer.what;
        if !answer.content_type.as_deref().is_some_and(|t| t.contains("json")) {
            return Err(CliError::Upstream(format!(
                "{what} answered HTTP {} with {}, not JSON",
                answer.status,
                answer.content_type.as_deref().unwrap_or("no content type")
            )));
        }
        let doc: Value = serde_json::from_slice(&answer.body)
            .map_err(|_| CliError::Upstream(format!("{what} answered JSON that does not parse")))?;
        signed_out(&doc, what, &self.profile)?;
        Ok(doc)
    }

    /// The session's cookie header as the answers left it, when one changed it.
    pub fn updated_cookie_header(&self) -> Option<Secret> {
        let jar = self.jar.borrow();
        jar.changed().then(|| Secret::new(jar.header()))
    }

    fn get(&self, path: &str) -> Result<Answer, CliError> {
        let what = format!("GET {}", crate::site::path_of(path));
        if let Some(last) = self.last.get() {
            let since = last.elapsed();
            if since < self.pace {
                std::thread::sleep(self.pace - since);
            }
        }
        let mut cookie = HeaderValue::from_str(&self.jar.borrow().header())
            .map_err(|_| CliError::Other("the session's Cookie header is not a valid HTTP header value".into()))?;
        cookie.set_sensitive(true);
        let result = self
            .http
            .get(format!("{}{path}", self.base))
            .header(ACCEPT, "application/json")
            .header(COOKIE, cookie)
            .send();
        let answer = match result {
            Ok(a) => a,
            Err(e) => {
                self.last.set(Some(Instant::now()));
                return Err(if e.is_timeout() {
                    CliError::Upstream(format!("{what} timed out after {} s", TIMEOUT.as_secs()))
                } else {
                    CliError::Upstream(format!("{what} got no answer from www.oneblood.org"))
                });
            }
        };
        let status = answer.status().as_u16();
        let header = |name| {
            answer
                .headers()
                .get(name)
                .and_then(|v: &HeaderValue| v.to_str().ok())
                .map(str::to_string)
        };
        let content_type = header(CONTENT_TYPE);
        let retry_after = header(RETRY_AFTER);
        {
            let mut jar = self.jar.borrow_mut();
            for v in answer.headers().get_all(SET_COOKIE) {
                if let Ok(v) = v.to_str() {
                    jar.apply(v);
                }
            }
        }
        let mut body = Vec::new();
        let read = answer.take(MAX_JSON + 1).read_to_end(&mut body);
        self.last.set(Some(Instant::now()));
        if read.is_err() {
            return Err(CliError::Upstream(format!(
                "{what} answered HTTP {status} and its body did not arrive"
            )));
        }
        if body.len() as u64 > MAX_JSON {
            return Err(CliError::Upstream(format!(
                "{what} answered with more than {} MiB",
                MAX_JSON >> 20
            )));
        }
        Ok(Answer {
            what,
            status,
            content_type,
            retry_after,
            body,
        })
    }
}

/// The paths `oneblood api` reads. `GET`s only, and only of the site's
/// donor and drive data: the sign-in paths (`/api/invita/session/`) are
/// refused, and so is any query naming an action (`create_…`, `cancel_…`),
/// which the site's own pages send only as `POST`s.
const API_PREFIXES: &[&str] = &[
    "/api/invita/mobileuser/",
    "/api/invita/mobilecenter/",
    "/api/invita/mobiledrive/",
    "/api/invita/maps/",
];
const ACTION_WORDS: &[&str] = &[
    "create",
    "cancel",
    "update",
    "delete",
    "reschedule",
    "book",
    "send",
    "login",
    "logout",
];

/// `oneblood api GET <PATH>`'s path on [`BASE`], or exit 2. Nothing is sent
/// before this passes.
pub fn api_path(path: &str) -> Result<String, CliError> {
    if path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CliError::Usage("the path holds a space or a control character".into()));
    }
    let path = match path.strip_prefix(BASE) {
        Some(rest) => rest,
        None if path.contains("://") => {
            return Err(CliError::Usage(format!(
                "`{BIN} api` reads only {BASE}; give a path (`/api/invita/...`) or a URL on it"
            )))
        }
        None => path,
    };
    let full = path.to_string();
    if !full.starts_with('/') {
        return Err(CliError::Usage("the path starts with `/`".into()));
    }
    let (route, query) = full.split_once('?').unwrap_or((&full, ""));
    let allowed = route == USER || API_PREFIXES.iter().any(|p| route.starts_with(p));
    if !allowed || route.contains("..") {
        return Err(CliError::Usage(format!(
            "`{BIN} api` reads the donor portal's data paths only: {USER} and paths under {}",
            API_PREFIXES.join(", ")
        )));
    }
    let names_an_action = query.split('&').any(|pair| {
        let key = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
        ACTION_WORDS.iter().any(|w| key.starts_with(w))
    });
    if names_an_action {
        return Err(CliError::Usage(format!(
            "{BIN} is read-only: that query names an action, which the site takes only as a POST"
        )));
    }
    Ok(full)
}

/// An answer, read whole.
pub struct Answer {
    /// `GET /path`, no query.
    pub what: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

fn login_remedy(profile: &ProfileName) -> String {
    format!("{BIN} auth login --browser --profile {profile} --overwrite")
}

fn refused_session(what: &str, said: &str, profile: &ProfileName) -> CliError {
    CliError::Auth(format!(
        "OneBlood refused profile `{profile}`'s session ({what}: {said}): it has signed out or expired"
    ))
    .because(Reason::CredentialRefused)
    .with_remedy(login_remedy(profile))
}

/// The answers that are no data whatever the read: a refused session, the
/// edge's page, too many requests, the site's own failures.
fn refuse(a: &Answer, profile: &ProfileName) -> Result<(), CliError> {
    let what = &a.what;
    let status = a.status;
    let html = a.content_type.as_deref().is_some_and(|t| t.contains("html"));
    match status {
        200..=299 => Ok(()),
        401 => Err(refused_session(what, "HTTP 401", profile)),
        300..=399 => Err(refused_session(what, &format!("a redirect, HTTP {status}"), profile)),
        403 if html => Err(CliError::Upstream(format!(
            "the site's edge refused {what} (HTTP 403 with its error page)"
        ))
        .because(Reason::ClientBlocked)
        .with_remedy(format!(
            "sign in again with `{}`, which keeps the browser's user agent",
            login_remedy(profile)
        ))),
        403 => Err(CliError::Upstream(format!("OneBlood refused {what} (HTTP 403)")).because(Reason::Forbidden)),
        429 => {
            let e = CliError::Upstream(format!("OneBlood answered {what} with HTTP 429: too many requests"));
            Err(match &a.retry_after {
                Some(after) => e.with_remedy(format!("try again after {} s", after.trim())),
                None => e.with_remedy("try again in a few minutes"),
            })
        }
        500..=599 => Err(
            CliError::Upstream(format!("OneBlood answered {what} with HTTP {status}")).with_remedy("try again later"),
        ),
        400 => Err(CliError::Upstream(format!(
            "OneBlood refused {what} as a bad request (HTTP 400); the site may have changed"
        ))
        .because(Reason::BadRequest)),
        _ => Err(CliError::Upstream(format!(
            "OneBlood answered {what} with HTTP {status}"
        ))),
    }
}

/// An answer that says, in its body, that nobody is signed in: `status`
/// `error` with a 401 in its `metadata`.
fn signed_out(doc: &Value, what: &str, profile: &ProfileName) -> Result<(), CliError> {
    let error = doc.get("status").and_then(Value::as_str) == Some("error");
    let code = doc.pointer("/metadata/status_code").and_then(Value::as_u64);
    if error && matches!(code, Some(401) | Some(403)) {
        return Err(refused_session(what, "not signed in", profile));
    }
    Ok(())
}

/// The site's error text, for a message: its machine word
/// (`error_message`, `reason`), cut to 120 characters. Never a value from
/// the donor's record: the site's errors carry none.
fn error_word(doc: &Value) -> Option<String> {
    ["/error_message", "/reason", "/metadata/message"]
        .iter()
        .find_map(|p| doc.pointer(p).and_then(Value::as_str))
        .map(|m| m.chars().take(120).collect())
}

/// An answer's `data`, or why there is none: `{"status": "success",
/// "data": ...}` is the data; an error answer is exit 3 when it says nobody
/// is signed in, otherwise exit 5 naming the site's own error word.
pub fn judge(a: &Answer, profile: &ProfileName) -> Result<Value, CliError> {
    refuse(a, profile)?;
    let what = &a.what;
    if !a.content_type.as_deref().is_some_and(|t| t.contains("json")) {
        return Err(CliError::Upstream(format!(
            "{what} answered HTTP {} without JSON",
            a.status
        )));
    }
    let doc: Value = serde_json::from_slice(&a.body)
        .map_err(|_| CliError::Upstream(format!("{what} answered JSON that does not parse")))?;
    signed_out(&doc, what, profile)?;
    match (doc.get("status").and_then(Value::as_str), doc.get("data")) {
        (Some("success"), Some(data)) => Ok(data.clone()),
        (Some(status), _) => Err(CliError::Upstream(match error_word(&doc) {
            Some(word) => format!("{what} answered `{status}`: {word}"),
            None => format!("{what} answered `{status}` without data"),
        })),
        (None, _) => Err(CliError::Upstream(format!(
            "{what} answered JSON without a status; the site may have changed"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adult1() -> ProfileName {
        ProfileName::parse("adult1").unwrap()
    }

    #[test]
    fn the_base_is_the_site_unless_a_test_points_it_at_loopback() {
        assert_eq!(base_from(None, false).unwrap(), BASE);
        assert_eq!(base_from(Some("  "), false).unwrap(), BASE);
        for ok in [
            "http://127.0.0.1:8080",
            "http://127.0.0.1:8080/",
            "http://localhost:9",
            "http://[::1]:9",
        ] {
            let b = base_from(Some(ok), true).unwrap();
            assert!(!b.ends_with('/'), "{b}");
        }
        // Without a test's own store, a real session could travel.
        assert_eq!(
            base_from(Some("http://127.0.0.1:8080"), false).unwrap_err().exit_code(),
            2
        );
        for bad in [
            "https://www.oneblood.org.example.com",
            "http://example.com",
            "http://127.0.0.1:1@example.com",
            "http://user@localhost:8",
            "https://127.0.0.1:8",
            "http://127.0.0.1:8/api",
            "http://127.0.0.1:8/?x=1",
            "http://127.0.0.2:8",
            "127.0.0.1:8",
        ] {
            assert_eq!(base_from(Some(bad), true).unwrap_err().exit_code(), 2, "{bad}");
        }
    }

    fn answer(status: u16, content_type: &str, body: &str) -> Answer {
        Answer {
            what: "GET /api/invita/mobileuser/resource".into(),
            status,
            content_type: Some(content_type.into()),
            retry_after: None,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn judge_reads_the_sites_answers() {
        let json = "application/json";
        let data = judge(&answer(200, json, r#"{"status":"success","data":{"a":1}}"#), &adult1()).unwrap();
        assert_eq!(data["a"], 1);
        let list = judge(&answer(200, json, r#"{"status":"success","data":[]}"#), &adult1()).unwrap();
        assert!(list.as_array().unwrap().is_empty());

        for (status, mime, body, exit) in [
            (401, json, "{}", 3),
            (302, "text/html", "", 3),
            (
                200,
                json,
                r#"{"status":"error","metadata":{"status_code":401},"error_message":"x"}"#,
                3,
            ),
            (
                200,
                json,
                r#"{"status":"error","metadata":{"status_code":500},"error_message":"invita_default_error"}"#,
                5,
            ),
            (200, json, r#"{"data":{}}"#, 5),
            (200, "text/html", "<p>", 5),
            (200, json, "{", 5),
            (403, "text/html", "<p>", 5),
            (429, json, "{}", 5),
            (503, json, "{}", 5),
            (400, json, "{}", 5),
        ] {
            let e = judge(&answer(status, mime, body), &adult1()).err().unwrap();
            assert_eq!(e.exit_code(), exit, "{status} {body}: {e}");
        }
        let e = judge(
            &answer(
                200,
                json,
                r#"{"status":"error","metadata":{"status_code":500},"error_message":"invita_default_error"}"#,
            ),
            &adult1(),
        )
        .err()
        .unwrap();
        assert!(e.message().contains("invita_default_error"), "{e}");
        let e = judge(&answer(401, json, "{}"), &adult1()).err().unwrap();
        assert!(e.to_string().contains("auth login --browser --profile adult1"), "{e}");
    }

    #[test]
    fn set_cookie_updates_the_jar() {
        let mut jar = Jar::parse("a=1; b=2");
        jar.apply("a=1; Path=/");
        assert!(!jar.changed(), "the same value changes nothing");
        jar.apply("a=3; Path=/; Secure");
        jar.apply("c=4");
        jar.apply("b=; Max-Age=0");
        assert!(jar.changed());
        assert_eq!(jar.header(), "a=3; c=4");
        assert_eq!(jar.get("c"), Some("4"));
        jar.apply("c=5; Max-Age=-1");
        assert_eq!(jar.header(), "a=3");
    }

    #[test]
    fn the_api_reads_only_the_sites_data_paths() {
        for ok in [
            "/api/invita/mobileuser/resource",
            "/api/invita/mobileuser/resource/?tests=1",
            "/api/invita/mobileuser/resource/?donations=1",
            "https://www.oneblood.org/api/invita/mobileuser/resource/?appointments=1",
            "/api/invita/maps/drives/?method=zip&zip=00000",
        ] {
            assert!(api_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "/api/invita/session/login",
            "/api/invita/session/send_otp",
            "/content/oneblood/us/en/jcr:content.invita.json?type=POST_LOGOUT",
            "/api/invita/mobileuser/resource/?cancel_appointment=1",
            "/api/invita/mobilecenter/resource/?create_appointment_center=1",
            "https://example.com/api/invita/mobileuser/resource",
            "http://www.oneblood.org/api/invita/mobileuser/resource",
            "api/invita/mobileuser/resource",
            "/api/invita/mobileuser/../session/login",
            "/api/invita/mobileuser/resource ?tests=1",
            "/sign-in.html",
        ] {
            assert_eq!(api_path(bad).err().unwrap().exit_code(), 2, "{bad}");
        }
    }

    #[test]
    fn the_read_paths_are_the_sites_own() {
        assert_eq!(tests_path(9), "/api/invita/mobileuser/resource/?tests=9");
        for p in [USER, DONATIONS, &tests_path(1)] {
            assert!(api_path(p).is_ok(), "{p}");
        }
    }
}
