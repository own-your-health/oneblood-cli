//! The donor portal on www.oneblood.org: what `auth login --browser` reads
//! out of the Chrome window, and the one read that confirms a session before
//! it is stored.
//!
//! The site sets visitor cookies before anyone signs in, so a cookie in the
//! jar proves nothing. The capture waits for [`SESSION_COOKIE`], which the
//! site's own pages read to tell a signed-in donor, then [`confirm_session`]
//! makes one read with the jar: the donor's own account, which a signed-in
//! donor gets as `{"status": "success", "data": {...}}` and anyone else
//! does not (docs/api.md).

use std::io::Read;
use std::time::Duration;

use pk_cli_browser::{BrowserError, Capture, Captured, Confirm, HostRule};
use pk_cli_core::CliError;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE};

use crate::cli::BIN;

/// The provider's name in the browser crate's status lines and errors.
pub const LABEL: &str = "OneBlood";
/// The host whose pages the capture reads.
pub const HOST: &str = "www.oneblood.org";
/// The domain whose whole cookie jar is the session.
pub const DOMAIN: &str = "oneblood.org";
/// Where the Chrome window opens: the site's sign-in page.
pub const START_URL: &str = "https://www.oneblood.org/sign-in.html";
/// What signing in involves, for the opening status line.
pub const HINT: &str = "your OneBlood email address, password and the code OneBlood sends";

/// A cookie the site's pages read to tell a signed-in donor (`"true"`), so
/// the capture asks [`confirm_session`] only once it is there.
pub const SESSION_COOKIE: &str = "userLoggedIn";

/// The read that shows whether a cookie jar is a signed-in donor's: the
/// donor's own account, `GET`.
pub const SESSION_CHECK_URL: &str = "https://www.oneblood.org/api/invita/mobileuser/resource";

/// The bound on the whole read, connecting included. The browser crate's poll
/// (and Ctrl-C) waits for it.
const TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of an answer's body read for [`judge`].
const MAX_BODY: u64 = 1024 * 1024;

/// What `auth login --browser` reads: a page on [`HOST`], the whole
/// [`DOMAIN`] jar and the browser's user agent. With `verify`, the tab stays
/// open until [`confirm_candidate`] says the jar is a signed-in one.
pub fn capture(verify: bool) -> Result<Capture, BrowserError> {
    let capture = Capture::new(LABEL, HostRule::exact(HOST)?)
        .cookie(START_URL, SESSION_COOKIE)
        .cookie_jar(DOMAIN)
        .with_user_agent()
        .hint(HINT);
    Ok(if verify {
        capture.confirm(confirm_candidate)
    } else {
        capture
    })
}

/// The capture's confirm hook: [`confirm_session`] on the candidate's jar.
pub fn confirm_candidate(candidate: &Captured) -> Confirm {
    match candidate.cookie_header() {
        Some(header) => confirm_session(header.expose(), candidate.user_agent()),
        None => Confirm::NotYet("the browser holds no oneblood.org cookie yet".into()),
    }
}

/// One read of [`SESSION_CHECK_URL`] with `cookie_header`, judged by
/// [`judge`]. A 429, a 5xx, a 403 with an HTML page, no answer or an answer
/// that does not arrive whole is `Undecided`. A redirect is `NotYet`.
/// `user_agent` is the browser's, for a session captured from one.
pub fn confirm_session(cookie_header: &str, user_agent: Option<&str>) -> Confirm {
    confirm_at(SESSION_CHECK_URL, cookie_header, user_agent)
}

/// How the answer from [`SESSION_CHECK_URL`] shows a signed-in donor.
///
/// `Ready` for a JSON answer `{"status": "success", "data": {...}}`: only a
/// signed-in donor's account resolves. `NotYet` for a 4xx, or a JSON answer
/// whose `status` is `error` (the site's answer to a visitor). Undecided for
/// a 2xx that is not JSON or says neither. It sees only answers that
/// [`confirm_session`] has not already decided: no 429, 5xx, 403 with an
/// HTML page or redirect. `body` is at most 1 MiB.
pub fn judge(status: u16, content_type: Option<&str>, body: &[u8]) -> Confirm {
    let what = format!("GET {}", path_of(SESSION_CHECK_URL));
    if (400..500).contains(&status) {
        return Confirm::NotYet(format!("{what} answered HTTP {status}: not signed in"));
    }
    if !(200..300).contains(&status) {
        return Confirm::undecided(format!("{what} answered HTTP {status}"));
    }
    if !content_type.is_some_and(|t| t.contains("json")) {
        return Confirm::undecided(format!("{what} answered HTTP {status} without JSON"));
    }
    let Ok(doc) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Confirm::undecided(format!("{what} answered HTTP {status} with JSON that does not parse"));
    };
    match doc.get("status").and_then(serde_json::Value::as_str) {
        Some("success") if doc.get("data").is_some_and(serde_json::Value::is_object) => Confirm::Ready,
        Some("error") => Confirm::NotYet(format!("{what} answered an error: not signed in")),
        _ => Confirm::undecided(format!("{what} answered without the donor's account")),
    }
}

/// [`confirm_session`]'s read of `url`, apart from the constant so the tests
/// can point it at a local server.
fn confirm_at(url: &str, cookie_header: &str, user_agent: Option<&str>) -> Confirm {
    let what = format!("GET {}", path_of(url));
    let client = match client(user_agent) {
        Ok(client) => client,
        Err(e) => return Confirm::Fail(e),
    };
    let Ok(mut cookie) = HeaderValue::from_str(cookie_header) else {
        return Confirm::Fail(CliError::Other(
            "the session's Cookie header is not a valid HTTP header value".into(),
        ));
    };
    cookie.set_sensitive(true);
    let answer = match client
        .get(url)
        .header(COOKIE, cookie)
        .header(ACCEPT, "application/json")
        .send()
    {
        Ok(answer) => answer,
        Err(e) if e.is_timeout() => return Confirm::undecided(format!("{what} timed out")),
        Err(_) => return Confirm::undecided(format!("{what} got no answer")),
    };
    if let Some(said) = pk_cli_http::inconclusive(&answer) {
        return Confirm::Undecided {
            reason: format!("{what} answered HTTP {}", said.status),
            retry_after: said.retry_after,
        };
    }
    let status = answer.status().as_u16();
    if answer.status().is_redirection() {
        return Confirm::NotYet(format!(
            "{what} answered with a redirect (HTTP {status}): not signed in"
        ));
    }
    let content_type = answer
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut body = Vec::new();
    if answer.take(MAX_BODY + 1).read_to_end(&mut body).is_err() {
        return Confirm::undecided(format!("{what} answered HTTP {status} and its body did not arrive"));
    }
    if body.len() as u64 > MAX_BODY {
        return Confirm::undecided(format!("{what} answered with more than 1 MiB"));
    }
    judge(status, content_type.as_deref(), &body)
}

/// The confirm read's client: no redirects followed, bounded in time.
fn client(user_agent: Option<&str>) -> Result<reqwest::blocking::Client, CliError> {
    let mut builder = pk_cli_http::builder(BIN, env!("CARGO_PKG_VERSION"))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(agent) = user_agent {
        builder = builder.user_agent(agent);
    }
    builder
        .build()
        .map_err(|e| CliError::Other(format!("building the HTTP client: {}", e.without_url())))
}

/// A URL's path without its query, for a reason: reasons never carry a query.
pub(crate) fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    path.split(['?', '#']).next().unwrap_or("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pk_cli_browser::JarCookie;
    use std::io::Write;
    use std::net::TcpListener;

    /// The capture `pk-cli-browser` checks before Chrome starts, checked
    /// offline, with and without the confirm hook.
    #[test]
    fn the_browser_capture_is_well_formed() {
        for verify in [true, false] {
            let capture = capture(verify).unwrap();
            capture.validate().unwrap();
            assert_eq!(capture.label(), LABEL);
        }
        assert!(START_URL.starts_with(&format!("https://{HOST}/")));
    }

    /// The session check stays on the site's own host, over https.
    #[test]
    fn the_session_check_is_an_https_read_on_oneblood_org() {
        let host = SESSION_CHECK_URL
            .strip_prefix("https://")
            .unwrap()
            .split(['/', '?', ':'])
            .next()
            .unwrap();
        assert_eq!(host, HOST);
    }

    fn candidate(jar: bool) -> Captured {
        let built = Captured::for_test(LABEL, HOST).user_agent("Mozilla/5.0 (test)");
        if jar {
            built.jar([
                JarCookie::for_test("dummy_visitor", "dummy-visitor", ".oneblood.org", false, "/"),
                JarCookie::for_test("dummy_session", "dummy-value", "www.oneblood.org", true, "/"),
            ])
        } else {
            built
        }
        .build()
    }

    /// A candidate without a jar is not read at all.
    #[test]
    fn no_jar_is_not_signed_in_yet() {
        match confirm_candidate(&candidate(false)) {
            Confirm::NotYet(reason) => assert!(reason.contains("no oneblood.org cookie"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    /// What the site answers (docs/api.md), judged.
    #[test]
    fn judge_reads_the_sites_answers() {
        let json = Some("application/json");
        let signed_in = br#"{"status":"success","data":{"donor_id":"dummy"}}"#;
        assert!(matches!(judge(200, json, signed_in), Confirm::Ready));
        let visitor = br#"{"status":"error","data":null}"#;
        assert!(matches!(judge(200, json, visitor), Confirm::NotYet(r) if r.contains("not signed in")));
        assert!(matches!(judge(401, json, b"{}"), Confirm::NotYet(r) if r.contains("HTTP 401")));
        for (status, mime, body) in [
            (200, json, &br#"{"status":"success","data":null}"#[..]),
            (200, json, &br#"{"other":1}"#[..]),
            (200, json, &b"{"[..]),
            (200, Some("text/html"), &b"<p>edge</p>"[..]),
            (304, None, &b""[..]),
        ] {
            assert!(
                matches!(judge(status, mime, body), Confirm::Undecided { .. }),
                "{status} {:?}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn a_reason_names_the_path_without_its_query() {
        assert_eq!(path_of("https://www.oneblood.org/api/x?tests=1#y"), "/api/x");
        assert_eq!(path_of("https://www.oneblood.org"), "/");
        assert_eq!(path_of("http://127.0.0.1:9/a/b"), "/a/b");
    }

    /// Serves one canned answer on loopback and hands back the request's head.
    fn serve_once(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/check?q=1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap() == 1 {
                head.push(byte[0]);
            }
            stream.write_all(answer.as_bytes()).unwrap();
            String::from_utf8_lossy(&head).to_lowercase()
        });
        (url, handle)
    }

    /// The read's own decisions, before [`judge`]: what cannot decide is
    /// `Undecided` (with the provider's `Retry-After`), a redirect is
    /// `NotYet`, and the rest reaches `judge`. The request carries the jar and
    /// the browser's user agent, and follows no redirect.
    #[test]
    fn the_confirm_read_decides_what_says_nothing_about_the_session() {
        let undecided = |answer: &'static str, retry: Option<u64>| {
            let (url, server) = serve_once(answer);
            match confirm_at(&url, "a=b; c=d", Some("Mozilla/5.0 (test)")) {
                Confirm::Undecided { reason, retry_after } => {
                    assert!(reason.starts_with("GET /api/check answered HTTP"), "{reason}");
                    assert!(!reason.contains("q=1"), "{reason}");
                    assert_eq!(retry_after, retry.map(Duration::from_secs));
                }
                other => panic!("{answer:?}: {other:?}"),
            }
            server.join().unwrap()
        };
        let head = undecided("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n", None);
        assert!(head.starts_with("get /api/check?q=1 "), "{head}");
        assert!(head.contains("\r\ncookie: a=b; c=d\r\n"), "{head}");
        assert!(head.contains("\r\nuser-agent: mozilla/5.0 (test)\r\n"), "{head}");
        undecided(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 30\r\nContent-Length: 0\r\n\r\n",
            Some(30),
        );
        undecided(
            "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 9\r\n\r\n<p>no</p>",
            None,
        );

        let (url, server) =
            serve_once("HTTP/1.1 302 Found\r\nLocation: https://example.com/sign-in.html\r\nContent-Length: 0\r\n\r\n");
        match confirm_at(&url, "a=b", None) {
            Confirm::NotYet(reason) => assert!(reason.contains("redirect (HTTP 302)"), "{reason}"),
            other => panic!("{other:?}"),
        }
        let head = server.join().unwrap();
        assert!(head.contains(&format!("\r\nuser-agent: {BIN}/")), "{head}");

        let (url, server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 37\r\n\r\n{\"status\":\"success\",\"data\":{\"a\":\"b\"}}",
        );
        assert!(matches!(confirm_at(&url, "a=b", None), Confirm::Ready));
        server.join().unwrap();
    }

    #[test]
    fn no_answer_is_undecided() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        match confirm_at(&format!("http://127.0.0.1:{port}/api/check"), "a=b", None) {
            Confirm::Undecided { reason, .. } => assert_eq!(reason, "GET /api/check got no answer"),
            other => panic!("{other:?}"),
        }
    }
}
