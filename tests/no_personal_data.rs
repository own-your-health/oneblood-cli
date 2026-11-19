//! The repository carries no personal data; runtime output may.
//!
//! Walks every tracked file (`git ls-files`) for the *shapes* personal data
//! takes (phone numbers, long id numbers, street addresses, email
//! addresses) rather than a list of real values, which would publish what
//! it exists to keep out. Then checks the fixtures positively: every
//! identity-bearing field holds one of the dummy shapes
//! `tests/fixtures/README.md` lists.

use std::process::Command;

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn tracked_files() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output()
        .expect("git ls-files");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn digits_run(s: &str, min: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            cur.push(c);
        } else {
            if cur.len() >= min {
                out.push(cur.clone());
            }
            cur.clear();
        }
    }
    out
}

/// A digit run is a dummy when it is all one digit, zeros then one digit
/// (`00000001`), or a 555 phone number: `555…`, or any area code with the
/// 555-0100 to 555-0199 lines reserved for fiction.
fn is_dummy_number(n: &str) -> bool {
    let first = n.chars().next().unwrap();
    let head = &n[..n.len() - 1];
    let local = n.strip_prefix('1').filter(|r| r.len() == 10).unwrap_or(n);
    let fictional = local.len() == 10 && &local[3..8] == "55501";
    n.chars().all(|c| c == first)
        || head.chars().all(|c| c == '0')
        || n.starts_with("555")
        || n.starts_with("1555")
        || fictional
}

/// Public code numbers the mapping writes: SNOMED CT's eight ABO and Rh
/// blood groups (src/fhir.rs, docs/fhir.md).
const PUBLIC_CODES: &[&str] = &[
    "278147001",
    "278148006",
    "278149003",
    "278150003",
    "278151004",
    "278152006",
    "278153001",
    "278154007",
];

const PUBLIC_EMAIL_DOMAINS: &[&str] = &["example.com", "oneblood.org"];

/// The personal-data shapes on one line, each as `kind value`.
fn scan_line(line: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let phoneish = line.replace(['(', ')', '-', '.', ' '], "");
    for n in digits_run(line, 7)
        .into_iter()
        .chain(digits_run(&phoneish, 10).into_iter().filter(|n| n.len() <= 11))
    {
        if !is_dummy_number(&n) && !PUBLIC_CODES.contains(&n.as_str()) {
            findings.push(format!("number {n}"));
        }
    }
    for word in line.split(|c: char| c.is_whitespace() || "\"'<>(),;`".contains(c)) {
        if let Some((local, domain)) = word.split_once('@') {
            let domain = domain.trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
            let ok = local.is_empty()
                || !domain.contains('.')
                || PUBLIC_EMAIL_DOMAINS.contains(&domain)
                || domain.ends_with(".example")
                || domain.ends_with("users.noreply.github.com");
            if !ok {
                findings.push(format!("email {word}"));
            }
        }
    }
    let lower = line.to_ascii_lowercase();
    for suffix in [" way", " st", " ave", " blvd", " dr", " ln", " ct", " rd"] {
        if let Some(pos) = lower.find(suffix) {
            let has_number = lower[..pos]
                .split_whitespace()
                .rev()
                .take(4)
                .any(|w| w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()));
            if has_number && !lower.contains("example st") {
                findings.push(format!("address-like `{}`", line.trim()));
            }
        }
    }
    findings
}

#[test]
fn tracked_files_carry_no_personal_data() {
    let files = tracked_files();
    if files.is_empty() {
        // Not a git checkout (a packaged crate): nothing to scan.
        return;
    }
    let mut findings = Vec::new();
    for file in files {
        if file.ends_with(".lock") || file.starts_with("LICENSE") || file == "tests/no_personal_data.rs" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(format!("{}/{file}", root())) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for f in scan_line(line) {
                findings.push(format!("{file}:{}: {f}", i + 1));
            }
        }
    }
    assert!(
        findings.is_empty(),
        "personal-data shapes in tracked files:\n{}",
        findings.join("\n")
    );
}

#[test]
fn no_tracked_file_is_a_raw_capture() {
    for file in tracked_files() {
        let lower = file.to_ascii_lowercase();
        for ext in [".har", ".csv", ".raw.json", ".raw.html", ".ndjson", ".png", ".jpg"] {
            assert!(!lower.ends_with(ext), "{file} is tracked; captures never are");
        }
    }
}

const FIRST_NAMES: &[&str] = &["Alex", "Sam", "Jordan"];

/// `prefix` then exactly `digits` digits.
fn shaped(value: &str, prefix: &str, digits: usize) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|d| d.len() == digits && d.chars().all(|c| c.is_ascii_digit()))
}

fn is_dummy_place(s: &str) -> bool {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "Example" || w == "Exampleton")
}

/// Every identity field the site fixtures carry, by key, wherever it is
/// nested: names are the dummies, ids the dummy shape, contact details
/// example ones, places fictional, dates of birth January firsts.
#[test]
fn identity_fields_in_site_fixtures_use_dummy_values() {
    fn walk(name: &str, key: &str, v: &serde_json::Value, checked: &mut usize) {
        match v {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    walk(name, k, v, checked);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(name, key, v, checked)),
            serde_json::Value::String(s) if !s.is_empty() => {
                let ok = match key {
                    "first_name" | "preferred_name" | "nickname" => FIRST_NAMES.contains(&s.as_str()),
                    "last_name" => s == "Example",
                    "middle_name" => s.len() == 1,
                    "donor_id" | "donor_number" => shaped(s, "SYND", 6),
                    "username" | "email" => s.ends_with("@example.com"),
                    "number" | "phone" => is_dummy_number(&s.replace(['(', ')', '-', ' '], "")),
                    "address" | "address1" | "address2" | "street" | "city" | "location" | "center_name" => {
                        is_dummy_place(s)
                    }
                    "zip" | "zip_code" | "postal_code" => s.chars().all(|c| c == '0'),
                    "birth_date" | "date_of_birth" | "dob" => s.ends_with("-01-01"),
                    _ => return,
                };
                assert!(ok, "{name}: `{key}` holds `{s}`, not a dummy");
                *checked += 1;
            }
            serde_json::Value::Number(n) => {
                if matches!(key, "donor_id" | "phone" | "number") {
                    let s = n.to_string();
                    assert!(is_dummy_number(&s), "{name}: `{key}` holds {s}, not a dummy");
                    *checked += 1;
                }
            }
            _ => {}
        }
    }
    let dir = format!("{}/tests/fixtures/site", root());
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.display().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        walk(&name, "", &v, &mut checked);
    }
    assert!(
        checked >= 8,
        "only {checked} identity fields found; did the fixtures change?"
    );
}

#[test]
fn the_scanner_flags_real_shapes_and_passes_dummies() {
    for real in [
        "call (772) 404-1234",
        "call 888-555-0200",
        "donor id 9234567812",
        "mail someone@gmail.com",
        "lives at 1234 NW Oak St, Somewhere",
        "donor 7342918",
    ] {
        assert!(!scan_line(real).is_empty(), "missed: {real}");
    }
    for dummy in [
        "call (555) 010-0001",
        "customer service 888-555-0100",
        "SYND000001",
        "alex@example.com",
        "100 Example St, Exampleton",
        "2025-03-14",
    ] {
        assert!(
            scan_line(dummy).is_empty(),
            "false positive: {dummy}: {:?}",
            scan_line(dummy)
        );
    }
    assert!(shaped("SYND000001", "SYND", 6));
    assert!(!shaped("SYND00001", "SYND", 6));
}
