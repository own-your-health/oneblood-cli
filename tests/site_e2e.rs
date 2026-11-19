//! Every command that reads the donor portal, in-process, against a
//! loopback server answering with `tests/fixtures/site/`, the session in an
//! in-memory vault: the requests' paths and headers, the DTOs and the
//! records stream, the cookie write-back, and the refusals. Nothing reads
//! the real keychain or reaches the network.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oneblood::model::Test;
use oneblood::portal::{self, Env, Site};
use oneblood::session::{Session, SessionStore};
use pk_cli_auth::vault::{MemoryKeys, Vault};
use pk_cli_config::profile::ProfileName;
use pk_cli_secrets::Secret;
use pk_med_records::{Item, RecordsReader};
use serde_json::Value;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/site/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// One request the server saw.
#[derive(Debug, Clone)]
struct Seen {
    target: String,
    cookie: Option<String>,
    user_agent: Option<String>,
}

/// A loopback server answering each `GET` by its path and query from
/// `routes`; anything else is a 404. `set_cookie` goes on every answer.
struct Server {
    base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn serve(routes: BTreeMap<String, (u16, String)>, set_cookie: Option<&'static str>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match stream.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let head = String::from_utf8_lossy(&head).to_string();
            let target = head.split_whitespace().nth(1).unwrap_or("").to_string();
            let header = |name: &str| {
                head.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
                })
            };
            log.lock().unwrap().push(Seen {
                target: target.clone(),
                cookie: header("cookie"),
                user_agent: header("user-agent"),
            });
            let (status, body) = routes.get(&target).cloned().unwrap_or((404, "{}".to_string()));
            let mut answer = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            if let Some(c) = set_cookie {
                answer.push_str(&format!("Set-Cookie: {c}\r\n"));
            }
            answer.push_str("\r\n");
            answer.push_str(&body);
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    Server { base, seen }
}

fn site_routes() -> BTreeMap<String, (u16, String)> {
    let mut r = BTreeMap::new();
    r.insert("/api/invita/mobileuser/resource".into(), (200, fixture("user.json")));
    r.insert(
        "/api/invita/mobileuser/resource/?donations=1".into(),
        (200, fixture("donations.json")),
    );
    for id in [1, 3, 4, 5, 6, 9] {
        r.insert(
            format!("/api/invita/mobileuser/resource/?tests={id}"),
            (200, fixture(&format!("tests-{id}.json"))),
        );
    }
    r
}

fn adult1() -> ProfileName {
    ProfileName::parse("adult1").unwrap()
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "oneblood-e2e-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// An environment reading `server`, with adult1's session stored.
fn env(server: &Server, tag: &str) -> Env {
    let store = SessionStore::with_vault(Vault::new(MemoryKeys::new(), temp_dir(tag)));
    store
        .put(
            &adult1(),
            &Session::new(
                Secret::new("dummy_session=1; userLoggedIn=true"),
                Some("Mozilla/5.0 (test)".into()),
            ),
        )
        .unwrap();
    Env {
        sessions: store,
        base: server.base.clone(),
        pace: Duration::ZERO,
    }
}

#[test]
fn patients_list_is_the_one_donor() {
    let server = serve(site_routes(), None);
    let env = env(&server, "patients");
    let site = Site::open(&env, &adult1()).unwrap();
    let v = portal::patients(&site).unwrap();
    assert_eq!(v["schema"], "patient-list/v1");
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], "SYND000001");
    assert_eq!(items[0]["organization"], "oneblood");
    assert_eq!(items[0]["relationship"], "self");
    assert_eq!(items[0]["name"], "Alex Example");
    assert_eq!(items[0]["blood_type"], "O Positive");
    let seen = server.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].target, "/api/invita/mobileuser/resource");
    assert_eq!(seen[0].cookie.as_deref(), Some("dummy_session=1; userLoggedIn=true"));
    assert_eq!(seen[0].user_agent.as_deref(), Some("Mozilla/5.0 (test)"));
}

fn read_stream(bytes: &[u8]) -> (pk_med_records::Header, Vec<Value>, pk_med_records::Trailer) {
    let mut reader = RecordsReader::new(std::io::BufReader::new(bytes));
    let header = reader.header().unwrap().clone();
    let mut resources = Vec::new();
    let mut trailer = None;
    for item in reader.by_ref() {
        match item.unwrap() {
            Item::Resource(r) => resources.push(r.value().unwrap()),
            Item::Trailer(t) => trailer = Some(t),
        }
    }
    (header, resources, trailer.unwrap())
}

#[test]
fn the_export_is_a_complete_stream_of_observations() {
    let server = serve(site_routes(), None);
    let env = env(&server, "export");
    let site = Site::open(&env, &adult1()).unwrap();
    let mut out = Vec::new();
    let finished = portal::export(&site, &mut out, Some("SYND000001"), None).unwrap();
    assert!(finished.trailer.complete);
    let (header, resources, trailer) = read_stream(&out);
    assert_eq!(header.organization.id, "oneblood");
    assert_eq!(header.patient.id, "SYND000001");
    assert_eq!(header.patient.name, None, "no name in the stream's header");
    assert_eq!(header.method(), "portal");
    let ids: Vec<&str> = resources.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids[..3], ["oneblood", "SYND000001", "blood-type"]);
    // 3 days, each: cholesterol, blood pressure, pulse, temperature, hemoglobin.
    assert_eq!(resources.len(), 3 + 3 * 5);
    assert_eq!(trailer.counts.get("Observation"), Some(&16));
    assert_eq!(trailer.counts.get("Patient"), Some(&1));
    assert!(trailer.skipped.is_empty());
    let bp = resources
        .iter()
        .find(|r| r["id"] == "blood-pressure-2025-03-14")
        .unwrap();
    assert_eq!(bp["component"][0]["valueQuantity"]["value"], 122);
    assert_eq!(bp["component"][1]["valueQuantity"]["value"], 80);
    let hgb = resources.iter().find(|r| r["id"] == "hemoglobin-2024-11-09").unwrap();
    assert_eq!(hgb["valueQuantity"]["value"], 13.9);
    assert_eq!(hgb["effectiveDateTime"], "2024-11-09");
    // The donor, then each of the six tests, in order.
    let targets: Vec<String> = server.seen.lock().unwrap().iter().map(|s| s.target.clone()).collect();
    assert_eq!(targets.len(), 7);
    assert_eq!(targets[0], "/api/invita/mobileuser/resource");
    for (i, test) in Test::ALL.iter().enumerate() {
        assert_eq!(
            targets[i + 1],
            format!("/api/invita/mobileuser/resource/?tests={}", test.id())
        );
    }
}

#[test]
fn since_bounds_the_readings_and_keeps_the_blood_type() {
    let server = serve(site_routes(), None);
    let env = env(&server, "since");
    let site = Site::open(&env, &adult1()).unwrap();
    let mut out = Vec::new();
    portal::export(&site, &mut out, None, Some("2025-03-14")).unwrap();
    let (header, resources, _) = read_stream(&out);
    assert_eq!(header.since.as_deref(), Some("2025-03-14"));
    assert!(resources.iter().any(|r| r["id"] == "blood-type"));
    assert_eq!(resources.len(), 3 + 2 * 5);
    assert!(resources
        .iter()
        .filter_map(|r| r["effectiveDateTime"].as_str())
        .all(|d| d >= "2025-03-14"));
}

#[test]
fn another_patient_is_refused_before_the_stream() {
    let server = serve(site_routes(), None);
    let env = env(&server, "patient");
    let site = Site::open(&env, &adult1()).unwrap();
    let mut out = Vec::new();
    let e = portal::export(&site, &mut out, Some("SYND000009"), None).unwrap_err();
    assert_eq!(e.exit_code(), 4);
    assert!(out.is_empty(), "nothing before the header");
}

#[test]
fn a_failure_after_the_header_ends_the_stream_incomplete() {
    let mut routes = site_routes();
    routes.insert("/api/invita/mobileuser/resource/?tests=5".into(), (503, "{}".into()));
    let server = serve(routes, None);
    let env = env(&server, "incomplete");
    let site = Site::open(&env, &adult1()).unwrap();
    let mut out = Vec::new();
    let finished = portal::export(&site, &mut out, None, None).unwrap();
    assert!(!finished.trailer.complete);
    let error = finished.trailer.error.as_ref().unwrap();
    assert_eq!(error.exit_code, Some(5));
    assert_eq!(oneblood::app::incomplete_exit(&finished.trailer), 5);
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().last().unwrap().contains("\"complete\":false"));
}

#[test]
fn a_signed_out_session_is_exit_3() {
    let mut routes = site_routes();
    routes.insert(
        "/api/invita/mobileuser/resource".into(),
        (200, fixture("signed-out.json")),
    );
    let server = serve(routes, None);
    let env = env(&server, "signed-out");
    let site = Site::open(&env, &adult1()).unwrap();
    let e = portal::patients(&site).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("auth login --browser --profile adult1"), "{e}");
    let mut routes = site_routes();
    routes.insert("/api/invita/mobileuser/resource".into(), (401, "{}".into()));
    let server = serve(routes, None);
    let env = self::env(&server, "401");
    let mut out = Vec::new();
    let e = portal::export(&Site::open(&env, &adult1()).unwrap(), &mut out, None, None).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(out.is_empty());
}

#[test]
fn readings_and_donations_lists() {
    let server = serve(site_routes(), None);
    let env = env(&server, "lists");
    let site = Site::open(&env, &adult1()).unwrap();
    let v = portal::readings_list(&site, &[Test::Cholesterol], Some("2025-01-01"), None).unwrap();
    assert_eq!(v["schema"], "reading-list/v1");
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["date"], "2025-08-02");
    assert_eq!(items[0]["test"], "cholesterol");
    assert_eq!(items[0]["value"], "176");
    assert_eq!(items[0]["unit"], "mg/dL");

    let v = portal::donations_list(&site, None, Some("2025-03-14")).unwrap();
    assert_eq!(v["schema"], "donation-list/v1");
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["date"], "2025-03-14");
    assert_eq!(items[0]["procedure"], "Platelets");
    assert_eq!(items[0]["mobile_drive"], true);
    assert_eq!(items[0]["blood_pressure"], "122/80");
    assert_eq!(items[0]["hemoglobin"], "13.6");
    assert_eq!(items[1]["mobile_drive"], false);
    assert!(v.get("unreadable").is_none());
}

#[test]
fn refreshed_cookies_are_written_back_once() {
    let server = serve(site_routes(), Some("dummy_session=2; Path=/; Secure; HttpOnly"));
    let env = env(&server, "writeback");
    let site = Site::open(&env, &adult1()).unwrap();
    portal::patients(&site).unwrap();
    assert!(site.finish().unwrap());
    let stored = env.sessions.get(&adult1()).unwrap().unwrap();
    assert_eq!(stored.cookie_header.expose(), "dummy_session=2; userLoggedIn=true");
    assert_eq!(stored.user_agent.as_deref(), Some("Mozilla/5.0 (test)"));

    // A sign-in made meanwhile is never overwritten.
    let site = Site::open(&env, &adult1()).unwrap();
    portal::patients(&site).unwrap();
    env.sessions
        .put(&adult1(), &Session::new(Secret::new("dummy_session=9"), None))
        .unwrap();
    assert!(!site.finish().unwrap());
    let stored = env.sessions.get(&adult1()).unwrap().unwrap();
    assert_eq!(stored.cookie_header.expose(), "dummy_session=9");
}

#[test]
fn the_api_returns_the_answer_whole() {
    let server = serve(site_routes(), None);
    let env = env(&server, "api");
    let site = Site::open(&env, &adult1()).unwrap();
    let path = oneblood::client::api_path("/api/invita/mobileuser/resource/?tests=1").unwrap();
    let v = site.api(&path).unwrap();
    assert_eq!(v["status"], "success");
    assert!(v["data"]["tests"].is_array());
}
