//! The donor portal's answers, read into oneblood's own types
//! (docs/api.md § Answers).
//!
//! Every reader takes the answer's `data` and refuses a shape it does not
//! know with exit 5 ("the site may have changed"), never quoting a value: a
//! message names fields, not what is in them. An item it cannot use (a
//! reading without a date, a donation without one) is counted, not
//! dropped silently, so a command can say how many it left out.

use pk_cli_core::CliError;
use serde_json::Value;

/// A test the donor portal charts, by the id its `?tests=` read takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Test {
    Cholesterol,
    Systolic,
    Diastolic,
    Pulse,
    Temperature,
    Hemoglobin,
}

impl Test {
    /// Every test, in the order the portal lists them.
    pub const ALL: [Test; 6] = [
        Test::Cholesterol,
        Test::Systolic,
        Test::Diastolic,
        Test::Pulse,
        Test::Temperature,
        Test::Hemoglobin,
    ];

    /// The id `/api/invita/mobileuser/resource/?tests=<id>` takes.
    pub fn id(self) -> u32 {
        match self {
            Test::Cholesterol => 1,
            Test::Systolic => 3,
            Test::Diastolic => 4,
            Test::Pulse => 5,
            Test::Temperature => 6,
            Test::Hemoglobin => 9,
        }
    }

    /// oneblood's name for it: the `test` column and field.
    pub fn name(self) -> &'static str {
        match self {
            Test::Cholesterol => "cholesterol",
            Test::Systolic => "systolic",
            Test::Diastolic => "diastolic",
            Test::Pulse => "pulse",
            Test::Temperature => "temperature",
            Test::Hemoglobin => "hemoglobin",
        }
    }

    /// The unit the portal charts it in, UCUM's spelling.
    pub fn unit(self) -> &'static str {
        match self {
            Test::Cholesterol => "mg/dL",
            Test::Systolic | Test::Diastolic => "mm[Hg]",
            Test::Pulse => "/min",
            Test::Temperature => "[degF]",
            Test::Hemoglobin => "g/dL",
        }
    }

    /// The unit as a person reads it.
    pub fn unit_display(self) -> &'static str {
        match self {
            Test::Cholesterol => "mg/dL",
            Test::Systolic | Test::Diastolic => "mmHg",
            Test::Pulse => "beats/min",
            Test::Temperature => "°F",
            Test::Hemoglobin => "g/dL",
        }
    }
}

/// The donor the session belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Donor {
    /// OneBlood's donor id: the `Patient.id` and `PatientContext.id`.
    pub id: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    /// As the portal words it: `A Positive`, `O Negative`.
    pub blood_type: Option<String>,
}

impl Donor {
    /// The name the portal shows, for the donor's own terminal only.
    pub fn name(&self) -> Option<String> {
        let parts: Vec<&str> = [self.first_name.as_deref(), self.last_name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// One reading of one test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub test: Test,
    /// The donation's day, `YYYY-MM-DD`.
    pub date: String,
    /// The value as the site served it: a decimal (`182`, `98.6`), or the
    /// site's own text when it is not one.
    pub value: String,
}

impl Reading {
    /// Whether [`Reading::value`] is a plain decimal.
    pub fn is_number(&self) -> bool {
        is_decimal(&self.value)
    }
}

/// One donation, or a booked appointment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Donation {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// What was given: `Whole Blood`, `Platelets`.
    pub procedure: Option<String>,
    /// The center or the drive's place.
    pub location: Option<String>,
    /// A donation center's id; `None` (`"0"` at the site) for a mobile drive.
    pub center_id: Option<String>,
}

/// What a reader read, and how many items it could not use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read<T> {
    pub items: Vec<T>,
    pub unreadable: usize,
}

fn changed(what: &str) -> CliError {
    CliError::Upstream(format!(
        "the donor portal's {what} is not the shape oneblood reads; the site may have changed"
    ))
}

/// A field as text: a string, or a number written as JSON writes it.
fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `YYYY-MM-DD` from a date the site writes as `YYYY-MM-DD`, optionally with
/// a time after it (`T…` or a space).
pub fn day(s: &str) -> Option<String> {
    let d = s.trim().get(..10)?;
    let rest = &s.trim()[10..];
    let b = d.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (rest.is_empty() || rest.starts_with('T') || rest.starts_with(' '));
    ok.then(|| d.to_string())
}

/// A plain decimal: digits, at most one point, an optional leading minus.
pub fn is_decimal(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    !whole.is_empty()
        && whole.bytes().all(|c| c.is_ascii_digit())
        && (s.find('.').is_none() || (!frac.is_empty() && frac.bytes().all(|c| c.is_ascii_digit())))
}

/// The donor's account: `GET /api/invita/mobileuser/resource`'s `data`.
pub fn donor(data: &Value) -> Result<Donor, CliError> {
    let obj = data.as_object().ok_or_else(|| changed("account answer"))?;
    // A donor's id is `donor_id`; the site's own pages fall back to `id`
    // only for someone who has not donated yet (a "prospect").
    let id = text(obj.get("donor_id")).filter(|id| id != "0").ok_or_else(|| {
        CliError::NotFound("the account has no donor id: OneBlood lists it once a first donation is recorded".into())
    })?;
    if !crate::fhir::is_id(&id) {
        return Err(changed("donor id"));
    }
    Ok(Donor {
        id,
        first_name: text(obj.get("first_name")),
        last_name: text(obj.get("last_name")),
        blood_type: text(obj.get("blood_type")),
    })
}

/// One test's history: `GET …/?tests=<id>`'s `data`, `{"tests": [{"test_date",
/// "value"}, …]}` (or the list alone). A reading without a date or value is
/// counted in `unreadable`.
pub fn readings(test: Test, data: &Value) -> Result<Read<Reading>, CliError> {
    let list = match data {
        Value::Array(list) => list,
        Value::Object(obj) => match obj.get("tests") {
            Some(Value::Array(list)) => list,
            Some(Value::Null) | None => {
                return Ok(Read {
                    items: Vec::new(),
                    unreadable: 0,
                })
            }
            Some(_) => return Err(changed("test history")),
        },
        Value::Null => {
            return Ok(Read {
                items: Vec::new(),
                unreadable: 0,
            })
        }
        _ => return Err(changed("test history")),
    };
    let mut items = Vec::new();
    let mut unreadable = 0;
    for item in list {
        let date = item.get("test_date").and_then(Value::as_str).and_then(day);
        let value = text(item.get("value"));
        match (date, value) {
            (Some(date), Some(value)) => items.push(Reading { test, date, value }),
            _ => unreadable += 1,
        }
    }
    items.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(Read { items, unreadable })
}

/// The donations: `GET …/?donations=1`'s `data`, a list of `{date,
/// procedure, location, center_id}` (or `{"donations": [...]}`), newest
/// first. One without a date is counted in `unreadable`.
pub fn donations(data: &Value) -> Result<Read<Donation>, CliError> {
    let list = match data {
        Value::Array(list) => list,
        Value::Object(obj) => match obj.get("donations") {
            Some(Value::Array(list)) => list,
            Some(Value::Null) | None => {
                return Ok(Read {
                    items: Vec::new(),
                    unreadable: 0,
                })
            }
            Some(_) => return Err(changed("donation history")),
        },
        Value::Null => {
            return Ok(Read {
                items: Vec::new(),
                unreadable: 0,
            })
        }
        _ => return Err(changed("donation history")),
    };
    let mut items = Vec::new();
    let mut unreadable = 0;
    for item in list {
        let Some(date) = item.get("date").and_then(Value::as_str).and_then(day) else {
            unreadable += 1;
            continue;
        };
        items.push(Donation {
            date,
            procedure: text(item.get("procedure")),
            location: text(item.get("location")),
            center_id: text(item.get("center_id")).filter(|c| c != "0"),
        });
    }
    items.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(Read { items, unreadable })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_ids_are_the_portals() {
        let ids: Vec<u32> = Test::ALL.iter().map(|t| t.id()).collect();
        assert_eq!(ids, [1, 3, 4, 5, 6, 9]);
    }

    #[test]
    fn days_are_iso_dates() {
        assert_eq!(day("2024-03-05").as_deref(), Some("2024-03-05"));
        assert_eq!(day("2024-03-05T10:00:00").as_deref(), Some("2024-03-05"));
        assert_eq!(day("2024-03-05 9:00").as_deref(), Some("2024-03-05"));
        for bad in ["", "03/05/2024", "2024-3-5", "2024-03-05x", "202403"] {
            assert_eq!(day(bad), None, "{bad}");
        }
    }

    #[test]
    fn decimals_are_plain() {
        for ok in ["182", "98.6", "0", "-1.5", "13.0"] {
            assert!(is_decimal(ok), "{ok}");
        }
        for bad in ["", ".5", "5.", "1e3", "1.2.3", "<100", "12 ", "+1"] {
            assert!(!is_decimal(bad), "{bad}");
        }
    }

    #[test]
    fn a_donor_needs_a_donor_id() {
        let d = donor(
            &json!({"donor_id": "D0001", "first_name": "Dummy", "last_name": "Donor",
            "blood_type": "O Positive", "id": "9"}),
        )
        .unwrap();
        assert_eq!(d.id, "D0001");
        assert_eq!(d.name().as_deref(), Some("Dummy Donor"));
        assert_eq!(d.blood_type.as_deref(), Some("O Positive"));
        let numeric = donor(&json!({"donor_id": 1234})).unwrap();
        assert_eq!(numeric.id, "1234");
        assert_eq!(numeric.name(), None);
        assert_eq!(donor(&json!({"id": "9"})).unwrap_err().exit_code(), 4);
        assert_eq!(donor(&json!({"donor_id": "0"})).unwrap_err().exit_code(), 4);
        assert_eq!(donor(&json!([])).unwrap_err().exit_code(), 5);
        let e = donor(&json!({"donor_id": "has space"})).unwrap_err();
        assert_eq!(e.exit_code(), 5);
        assert!(!e.message().contains("has space"), "{e}");
    }

    #[test]
    fn readings_are_newest_first_and_count_what_they_leave() {
        let data = json!({"tests": [
            {"test_date": "2024-01-02", "value": "180"},
            {"test_date": "2025-06-07", "value": 175},
            {"test_date": "", "value": "1"},
            {"test_date": "2023-01-01", "value": null},
        ]});
        let r = readings(Test::Cholesterol, &data).unwrap();
        assert_eq!(r.unreadable, 2);
        assert_eq!(
            r.items,
            [
                Reading {
                    test: Test::Cholesterol,
                    date: "2025-06-07".into(),
                    value: "175".into()
                },
                Reading {
                    test: Test::Cholesterol,
                    date: "2024-01-02".into(),
                    value: "180".into()
                },
            ]
        );
        assert!(readings(Test::Pulse, &json!({"tests": []})).unwrap().items.is_empty());
        assert!(readings(Test::Pulse, &json!({})).unwrap().items.is_empty());
        assert!(readings(Test::Pulse, &json!(null)).unwrap().items.is_empty());
        assert_eq!(
            readings(Test::Pulse, &json!([{"test_date": "2024-01-01", "value": "60"}]))
                .unwrap()
                .items
                .len(),
            1
        );
        assert_eq!(readings(Test::Pulse, &json!("x")).unwrap_err().exit_code(), 5);
        assert_eq!(readings(Test::Pulse, &json!({"tests": 1})).unwrap_err().exit_code(), 5);
    }

    #[test]
    fn donations_are_newest_first() {
        let data = json!([
            {"date": "2024-01-02", "procedure": "Whole Blood", "location": "Dummy Center", "center_id": "12"},
            {"date": "2025-01-02", "procedure": "Platelets", "location": "Dummy Bus", "center_id": "0"},
            {"procedure": "Whole Blood"},
        ]);
        let r = donations(&data).unwrap();
        assert_eq!(r.unreadable, 1);
        assert_eq!(r.items[0].date, "2025-01-02");
        assert_eq!(r.items[0].center_id, None);
        assert_eq!(r.items[1].center_id.as_deref(), Some("12"));
        assert_eq!(donations(&json!({"donations": []})).unwrap().items.len(), 0);
        assert_eq!(donations(&json!(7)).unwrap_err().exit_code(), 5);
    }
}
