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

/// `line` with each `YYYY-MM-DD HH:MM:SS` (or `T` between) blanked: the
/// portal's own time stamps, which read as a phone number once the
/// separators are gone.
fn without_datetimes(line: &str) -> String {
    let b = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        let is = |j: usize, digits: bool| b.get(i + j).is_some_and(|c| c.is_ascii_digit() == digits);
        let at = |j: usize, c: u8| b.get(i + j) == Some(&c);
        let datetime = (0..4).all(|j| is(j, true))
            && at(4, b'-')
            && is(5, true)
            && is(6, true)
            && at(7, b'-')
            && is(8, true)
            && is(9, true)
            && (at(10, b' ') || at(10, b'T'))
            && is(11, true)
            && is(12, true)
            && at(13, b':')
            && is(14, true)
            && is(15, true)
            && at(16, b':')
            && is(17, true)
            && is(18, true);
        if datetime {
            out.push_str("<datetime>");
            i += 19;
        } else {
            let c = line[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The personal-data shapes on one line, each as `kind value`.
fn scan_line(line: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let line = without_datetimes(line);
    let line = line.as_str();
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
    // A street suffix as a whole word ("St", not "status"), anywhere on the
    // line, with a house number in the four words before it.
    let address_like = [" way", " st", " ave", " blvd", " dr", " ln", " ct", " rd"]
        .iter()
        .any(|suffix| {
            lower.match_indices(suffix).any(|(pos, _)| {
                let whole_word = !lower[pos + suffix.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric());
                whole_word
                    && lower[..pos]
                        .split_whitespace()
                        .rev()
                        .take(4)
                        .any(|w| w.len() >= 3 && w.chars().all(|c| c.is_ascii_digit()))
            })
        });
    if address_like && !lower.contains("example st") {
        findings.push(format!("address-like `{}`", line.trim()));
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

/// Zeros, then one digit (`0000001`): the shape the fixtures README gives
/// ids that are not phone numbers. A JSON number of one digit counts, its
/// zeros having been dropped.
fn zeros_then_digit(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit()) && !s.is_empty() && s[..s.len() - 1].bytes().all(|b| b == b'0')
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
                    // A blood unit's number (ISBT 128), on each reading and donation.
                    "unit_number" => shaped(s, "SYNU", 9),
                    "becs_id" | "sponsor_code" => zeros_then_digit(s),
                    "image_data" => s == "AAAA",
                    "token" => s == "example-token",
                    _ => return,
                };
                assert!(ok, "{name}: `{key}` holds `{s}`, not a dummy");
                *checked += 1;
            }
            serde_json::Value::Number(n) => {
                let s = n.to_string();
                let ok = match key {
                    "donor_id" | "phone" | "number" => is_dummy_number(&s),
                    "becs_id" | "sponsor_code" => zeros_then_digit(&s),
                    _ => return,
                };
                assert!(ok, "{name}: `{key}` holds {s}, not a dummy");
                *checked += 1;
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
        checked >= 40,
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
        "call 772-404-1234 at 2025-08-02 12:00:00",
        "the status was 200, then 1234 Oak St",
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
        "\"created\": \"2025-08-02 12:00:00\"",
        "one 401 and 302), drives every command",
    ] {
        assert!(
            scan_line(dummy).is_empty(),
            "false positive: {dummy}: {:?}",
            scan_line(dummy)
        );
    }
    assert!(zeros_then_digit("0000001"));
    assert!(zeros_then_digit("7"));
    for real in ["5551234", "7777777", "0001001", "", "000000x"] {
        assert!(!zeros_then_digit(real), "{real}");
    }
    assert!(shaped("SYND000001", "SYND", 6));
    assert!(!shaped("SYND00001", "SYND", 6));
}
