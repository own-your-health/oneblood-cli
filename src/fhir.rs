//! The donor portal's readings as FHIR R4 resources (docs/fhir.md), mapped
//! by hand: no FHIR library.
//!
//! - One `Organization` (OneBlood) and one `Patient` (the donor, by
//!   OneBlood's donor id).
//! - The blood type: an `Observation`, LOINC 882-1 (ABO and Rh group), its
//!   value a SNOMED CT blood group with the portal's own words as its text.
//! - Each reading an `Observation` dated by its donation: total
//!   cholesterol (2093-3), hemoglobin (718-7, the portal's "iron count"),
//!   pulse (8867-4) and temperature (8310-5); systolic and diastolic taken
//!   the same day are one blood pressure panel (85354-9) with two components
//!   (8480-6, 8462-4), as FHIR's vital signs profile asks.
//!
//! Ids are stable across runs (the test and the day), so a records store
//! merges the same reading pulled twice. Every resource carries
//! `meta.source` `oneblood:portal` and a tag saying it was mapped.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::model::{Donor, Reading, Test};

pub const ORGANIZATION_ID: &str = "oneblood";
pub const ORGANIZATION_NAME: &str = "OneBlood";
pub const SITE: &str = "https://www.oneblood.org";
pub const META_SOURCE: &str = "oneblood:portal";
pub const TAG_SYSTEM: &str = "https://github.com/piekstra/oneblood-cli/fhir/tag";
pub const TAG_MAPPED: &str = "mapped-from-donor-portal";
/// The donor id's identifier system: oneblood's own URI, since OneBlood
/// publishes none.
pub const DONOR_ID_SYSTEM: &str = "https://github.com/piekstra/oneblood-cli/fhir/donor-id";

const LOINC: &str = "http://loinc.org";
const SNOMED: &str = "http://snomed.info/sct";
const UCUM: &str = "http://unitsofmeasure.org";
const CATEGORY: &str = "http://terminology.hl7.org/CodeSystem/observation-category";
const ABSENT: &str = "http://terminology.hl7.org/CodeSystem/data-absent-reason";

/// FHIR's `id`: 1 to 64 of letters, digits, `-` and `.`.
pub fn is_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}

fn meta() -> Value {
    json!({
        "source": META_SOURCE,
        "tag": [{ "system": TAG_SYSTEM, "code": TAG_MAPPED, "display": "Mapped by oneblood from the OneBlood donor portal" }],
    })
}

fn coding(system: &str, code: &str, display: &str) -> Value {
    json!({ "system": system, "code": code, "display": display })
}

fn category(code: &str, display: &str) -> Value {
    json!([{ "coding": [coding(CATEGORY, code, display)] }])
}

fn loinc(code: &str, display: &str, text: &str) -> Value {
    json!({ "coding": [coding(LOINC, code, display)], "text": text })
}

pub fn organization() -> Value {
    json!({
        "resourceType": "Organization",
        "id": ORGANIZATION_ID,
        "meta": meta(),
        "active": true,
        "name": ORGANIZATION_NAME,
        "telecom": [{ "system": "url", "value": SITE }],
    })
}

/// The donor, with the name the portal shows when it shows one.
pub fn patient(d: &Donor) -> Value {
    let mut v = json!({
        "resourceType": "Patient",
        "id": d.id,
        "meta": meta(),
        "identifier": [{
            "system": DONOR_ID_SYSTEM,
            "value": d.id,
            "assigner": { "reference": format!("Organization/{ORGANIZATION_ID}"), "display": ORGANIZATION_NAME },
        }],
    });
    let mut name = Map::new();
    if let Some(family) = d.last_name.as_deref().filter(|s| !s.is_empty()) {
        name.insert("family".into(), family.into());
    }
    if let Some(given) = d.first_name.as_deref().filter(|s| !s.is_empty()) {
        name.insert("given".into(), json!([given]));
    }
    if let Some(text) = d.name() {
        name.insert("text".into(), text.into());
    }
    if !name.is_empty() {
        v["name"] = json!([Value::Object(name)]);
    }
    v
}

/// A blood type as the portal words it (`A Positive`, `AB Negative`, any
/// case, `+`/`-` too): the SNOMED CT blood group and its display.
pub fn blood_group(text: &str) -> Option<(&'static str, &'static str)> {
    let t = text.trim().to_ascii_lowercase().replace(['(', ')'], "");
    let (abo, rh) = if let Some(abo) = t.strip_suffix('+') {
        (abo.trim(), true)
    } else if let Some(abo) = t.strip_suffix('-') {
        (abo.trim(), false)
    } else {
        let (abo, rh) = t.split_once(' ')?;
        let rh = rh.trim().trim_start_matches("rh").trim();
        match rh {
            "positive" | "pos" => (abo.trim(), true),
            "negative" | "neg" => (abo.trim(), false),
            _ => return None,
        }
    };
    Some(match (abo, rh) {
        ("a", true) => ("278149003", "Blood group A Rh(D) positive"),
        ("a", false) => ("278152006", "Blood group A Rh(D) negative"),
        ("b", true) => ("278150003", "Blood group B Rh(D) positive"),
        ("b", false) => ("278153001", "Blood group B Rh(D) negative"),
        ("ab", true) => ("278151004", "Blood group AB Rh(D) positive"),
        ("ab", false) => ("278154007", "Blood group AB Rh(D) negative"),
        ("o", true) => ("278147001", "Blood group O Rh(D) positive"),
        ("o", false) => ("278148006", "Blood group O Rh(D) negative"),
        _ => return None,
    })
}

pub const BLOOD_TYPE_ID: &str = "blood-type";

/// The blood type as an Observation. The portal gives no date for it, so it
/// carries none. A type the mapping does not know keeps the portal's words
/// as the value's text, with no coding.
pub fn blood_type(d: &Donor) -> Option<Value> {
    let text = d.blood_type.as_deref()?.trim();
    if text.is_empty() {
        return None;
    }
    let value = match blood_group(text) {
        Some((code, display)) => json!({ "coding": [coding(SNOMED, code, display)], "text": text }),
        None => json!({ "text": text }),
    };
    Some(json!({
        "resourceType": "Observation",
        "id": BLOOD_TYPE_ID,
        "meta": meta(),
        "status": "final",
        "category": category("laboratory", "Laboratory"),
        "code": loinc("882-1", "ABO and Rh group [Type] in Blood", "Blood type"),
        "subject": subject(d),
        "performer": [performer()],
        "valueCodeableConcept": value,
    }))
}

fn subject(d: &Donor) -> Value {
    json!({ "reference": format!("Patient/{}", d.id) })
}

fn performer() -> Value {
    json!({ "reference": format!("Organization/{ORGANIZATION_ID}"), "display": ORGANIZATION_NAME })
}

/// A served decimal as a JSON number, its spelling kept as far as JSON
/// numbers go: an integer stays one (`182`), a fraction is a float (`98.6`).
fn number(value: &str) -> Option<Value> {
    if let Ok(n) = value.parse::<i64>() {
        return Some(n.into());
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite())
        .and_then(serde_json::Number::from_f64)
        .map(Value::Number)
}

fn quantity(test: Test, value: &str) -> Option<Value> {
    Some(json!({
        "value": number(value)?,
        "unit": test.unit_display(),
        "system": UCUM,
        "code": test.unit(),
    }))
}

/// A value: a quantity for a decimal, else the portal's text.
fn value_fields(test: Test, r: &Reading, into: &mut Map<String, Value>) {
    match r.is_number().then(|| quantity(test, &r.value)).flatten() {
        Some(q) => {
            into.insert("valueQuantity".into(), q);
        }
        None => {
            into.insert("valueString".into(), r.value.clone().into());
        }
    }
}

/// The resource id of a single test's reading on `date`.
pub fn reading_id(test: Test, date: &str) -> String {
    let name = match test {
        Test::Systolic | Test::Diastolic => "blood-pressure",
        other => other.name(),
    };
    format!("{name}-{date}")
}

fn single(d: &Donor, r: &Reading) -> Value {
    let (code, cat) = match r.test {
        Test::Cholesterol => (
            loinc(
                "2093-3",
                "Cholesterol [Mass/volume] in Serum or Plasma",
                "Total cholesterol",
            ),
            category("laboratory", "Laboratory"),
        ),
        Test::Hemoglobin => (
            loinc(
                "718-7",
                "Hemoglobin [Mass/volume] in Blood",
                "Hemoglobin (OneBlood's iron count)",
            ),
            category("laboratory", "Laboratory"),
        ),
        Test::Pulse => (
            loinc("8867-4", "Heart rate", "Pulse"),
            category("vital-signs", "Vital Signs"),
        ),
        Test::Temperature => (
            loinc("8310-5", "Body temperature", "Temperature"),
            category("vital-signs", "Vital Signs"),
        ),
        Test::Systolic | Test::Diastolic => unreachable!("blood pressure is a panel"),
    };
    let mut v = Map::new();
    v.insert("resourceType".into(), "Observation".into());
    v.insert("id".into(), reading_id(r.test, &r.date).into());
    v.insert("meta".into(), meta());
    v.insert("status".into(), "final".into());
    v.insert("category".into(), cat);
    v.insert("code".into(), code);
    v.insert("subject".into(), subject(d));
    v.insert("effectiveDateTime".into(), r.date.clone().into());
    v.insert("performer".into(), json!([performer()]));
    value_fields(r.test, r, &mut v);
    Value::Object(v)
}

fn component(test: Test, r: Option<&Reading>) -> Value {
    let code = match test {
        Test::Systolic => loinc("8480-6", "Systolic blood pressure", "Systolic"),
        _ => loinc("8462-4", "Diastolic blood pressure", "Diastolic"),
    };
    let mut c = Map::new();
    c.insert("code".into(), code);
    match r {
        Some(r) => value_fields(test, r, &mut c),
        None => {
            c.insert(
                "dataAbsentReason".into(),
                json!({ "coding": [coding(ABSENT, "unknown", "Unknown")] }),
            );
        }
    }
    Value::Object(c)
}

fn blood_pressure(d: &Donor, date: &str, systolic: Option<&Reading>, diastolic: Option<&Reading>) -> Value {
    json!({
        "resourceType": "Observation",
        "id": reading_id(Test::Systolic, date),
        "meta": meta(),
        "status": "final",
        "category": category("vital-signs", "Vital Signs"),
        "code": loinc("85354-9", "Blood pressure panel with all children optional", "Blood pressure"),
        "subject": subject(d),
        "effectiveDateTime": date,
        "performer": [performer()],
        "component": [component(Test::Systolic, systolic), component(Test::Diastolic, diastolic)],
    })
}

/// Every reading as Observations, newest first, one per test and day:
/// systolic and diastolic of a day are one panel. A second reading of the
/// same test on the same day (which the portal has not been seen to serve)
/// is left out and counted, since its id would repeat the first's.
pub fn readings(d: &Donor, readings: &[Reading]) -> (Vec<Value>, usize) {
    let mut by_key: BTreeMap<(String, Test), &Reading> = BTreeMap::new();
    let mut repeated = 0;
    for r in readings {
        if by_key.insert((r.date.clone(), r.test), r).is_some() {
            repeated += 1;
        }
    }
    let mut days: Vec<&String> = by_key.keys().map(|(date, _)| date).collect();
    days.dedup();
    let mut out = Vec::new();
    for date in days.into_iter().rev() {
        let get = |t: Test| by_key.get(&(date.clone(), t)).copied();
        for test in Test::ALL {
            match test {
                Test::Systolic => {
                    let (s, di) = (get(Test::Systolic), get(Test::Diastolic));
                    if s.is_some() || di.is_some() {
                        out.push(blood_pressure(d, date, s, di));
                    }
                }
                Test::Diastolic => {}
                t => {
                    if let Some(r) = get(t) {
                        out.push(single(d, r));
                    }
                }
            }
        }
    }
    (out, repeated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn donor() -> Donor {
        Donor {
            id: "D0001".into(),
            first_name: Some("Dummy".into()),
            last_name: Some("Donor".into()),
            blood_type: Some("O Positive".into()),
        }
    }

    fn reading(test: Test, date: &str, value: &str) -> Reading {
        Reading {
            test,
            date: date.into(),
            value: value.into(),
        }
    }

    #[test]
    fn blood_groups_are_snomed() {
        assert_eq!(blood_group("O Positive").unwrap().0, "278147001");
        assert_eq!(blood_group("o negative").unwrap().0, "278148006");
        assert_eq!(blood_group("A Positive").unwrap().0, "278149003");
        assert_eq!(blood_group("A Negative").unwrap().0, "278152006");
        assert_eq!(blood_group("B Positive").unwrap().0, "278150003");
        assert_eq!(blood_group("B Negative").unwrap().0, "278153001");
        assert_eq!(blood_group("AB Positive").unwrap().0, "278151004");
        assert_eq!(blood_group("AB Negative").unwrap().0, "278154007");
        assert_eq!(blood_group("AB+").unwrap().0, "278151004");
        assert_eq!(blood_group("O-").unwrap().0, "278148006");
        assert_eq!(blood_group("A Rh Positive").unwrap().0, "278149003");
        for bad in ["", "Unknown", "C Positive", "A", "A sideways"] {
            assert_eq!(blood_group(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_blood_type_keeps_the_portals_words() {
        let v = blood_type(&donor()).unwrap();
        assert_eq!(v["id"], BLOOD_TYPE_ID);
        assert_eq!(v["code"]["coding"][0]["code"], "882-1");
        assert_eq!(v["valueCodeableConcept"]["coding"][0]["code"], "278147001");
        assert_eq!(v["valueCodeableConcept"]["text"], "O Positive");
        assert!(v.get("effectiveDateTime").is_none());
        let odd = Donor {
            blood_type: Some("Pending".into()),
            ..donor()
        };
        let v = blood_type(&odd).unwrap();
        assert!(v["valueCodeableConcept"].get("coding").is_none());
        assert_eq!(v["valueCodeableConcept"]["text"], "Pending");
        assert!(blood_type(&Donor {
            blood_type: None,
            ..donor()
        })
        .is_none());
    }

    #[test]
    fn readings_are_observations_per_test_and_day() {
        let rs = [
            reading(Test::Cholesterol, "2025-01-02", "182"),
            reading(Test::Systolic, "2025-01-02", "120"),
            reading(Test::Diastolic, "2025-01-02", "80"),
            reading(Test::Temperature, "2025-01-02", "98.6"),
            reading(Test::Hemoglobin, "2024-06-07", "13.9"),
            reading(Test::Systolic, "2024-06-07", "118"),
            reading(Test::Pulse, "2024-06-07", "<40"),
        ];
        let (out, repeated) = readings(&donor(), &rs);
        assert_eq!(repeated, 0);
        let ids: Vec<&str> = out.iter().map(|v| v["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "cholesterol-2025-01-02",
                "blood-pressure-2025-01-02",
                "temperature-2025-01-02",
                "blood-pressure-2024-06-07",
                "pulse-2024-06-07",
                "hemoglobin-2024-06-07",
            ]
        );
        assert!(ids.iter().all(|id| is_id(id)));
        let chol = &out[0];
        assert_eq!(chol["valueQuantity"]["value"], 182);
        assert_eq!(chol["valueQuantity"]["code"], "mg/dL");
        assert_eq!(chol["effectiveDateTime"], "2025-01-02");
        assert_eq!(chol["subject"]["reference"], "Patient/D0001");
        assert_eq!(chol["meta"]["source"], META_SOURCE);
        let bp = &out[1];
        assert_eq!(bp["code"]["coding"][0]["code"], "85354-9");
        assert_eq!(bp["component"][0]["code"]["coding"][0]["code"], "8480-6");
        assert_eq!(bp["component"][0]["valueQuantity"]["value"], 120);
        assert_eq!(bp["component"][1]["valueQuantity"]["value"], 80);
        assert_eq!(bp["component"][1]["valueQuantity"]["code"], "mm[Hg]");
        assert_eq!(out[2]["valueQuantity"]["value"], 98.6);
        assert_eq!(out[2]["valueQuantity"]["code"], "[degF]");
        let half = &out[3];
        assert_eq!(half["component"][1]["dataAbsentReason"]["coding"][0]["code"], "unknown");
        assert_eq!(out[4]["valueString"], "<40");
        assert_eq!(out[5]["code"]["coding"][0]["code"], "718-7");
        assert_eq!(out[5]["valueQuantity"]["value"], 13.9);
    }

    #[test]
    fn a_repeated_reading_is_counted_not_written_twice() {
        let rs = [
            reading(Test::Pulse, "2025-01-02", "60"),
            reading(Test::Pulse, "2025-01-02", "62"),
        ];
        let (out, repeated) = readings(&donor(), &rs);
        assert_eq!((out.len(), repeated), (1, 1));
    }

    #[test]
    fn the_patient_is_the_donor() {
        let p = patient(&donor());
        assert_eq!(p["id"], "D0001");
        assert_eq!(p["identifier"][0]["value"], "D0001");
        assert_eq!(p["name"][0]["family"], "Donor");
        assert_eq!(p["name"][0]["given"][0], "Dummy");
        let bare = patient(&Donor {
            first_name: None,
            last_name: None,
            ..donor()
        });
        assert!(bare.get("name").is_none());
        assert_eq!(organization()["id"], ORGANIZATION_ID);
    }

    #[test]
    fn ids_follow_fhir() {
        assert!(is_id("blood-pressure-2025-01-02"));
        assert!(is_id("123456"));
        assert!(!is_id(""));
        assert!(!is_id("has space"));
        assert!(!is_id(&"x".repeat(65)));
    }
}
