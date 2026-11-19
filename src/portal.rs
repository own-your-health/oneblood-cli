//! The commands that read the donor portal as a profile: what they read,
//! and the DTOs and stream they make of it. `app` prints what these return;
//! the tests run them in-process against a loopback server with the session
//! in an in-memory vault ([`Env`]).

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use pk_cli_config::profile::ProfileName;
use pk_cli_core::CliError;
use pk_med_records::dto::{kind, method, relationship};
use pk_med_records::{Finished, Header, Organization, PatientContext, RawResource, RecordsWriter, Skip, Source};
use serde_json::{json, Value};

use crate::cli::BIN;
use crate::client::{self, Client};
use crate::fhir;
use crate::model::{self, Donation, Donor, Reading, Test};
use crate::session::{Session, SessionStore};

/// Where sessions are kept and which site is read.
pub struct Env {
    pub sessions: SessionStore,
    /// The site's origin, or a test's loopback server.
    pub base: String,
    /// The least time between one answer and the next request.
    pub pace: Duration,
}

impl Env {
    /// The binary's: the vault, www.oneblood.org, [`client::PACE`] apart.
    pub fn for_binary() -> Result<Env, CliError> {
        // The base first: a refused override reaches no vault.
        let base = client::base()?;
        Ok(Env {
            sessions: SessionStore::open()?,
            base,
            pace: client::PACE,
        })
    }

    /// Whether this reads the real site, as opposed to a test's server.
    pub fn is_real_site(&self) -> bool {
        self.base == client::BASE
    }
}

/// A profile's signed-in reads.
pub struct Site<'a> {
    client: Client,
    profile: ProfileName,
    session: Session,
    store: &'a SessionStore,
}

/// What a reading command read, beside the readings: how many items the
/// site served that could not be used, per test.
#[derive(Debug, Default)]
pub struct Readings {
    pub items: Vec<Reading>,
    pub unreadable: BTreeMap<Test, usize>,
}

impl<'a> Site<'a> {
    /// The profile's stored session (none is exit 3, naming the sign-in).
    pub fn open(env: &'a Env, profile: &ProfileName) -> Result<Site<'a>, CliError> {
        let session = env.sessions.load(profile)?;
        let client = Client::new(&session, profile, &env.base, env.pace)?;
        Ok(Site {
            client,
            profile: profile.clone(),
            session,
            store: &env.sessions,
        })
    }

    pub fn donor(&self) -> Result<Donor, CliError> {
        model::donor(&self.client.data(client::USER)?)
    }

    pub fn test(&self, test: Test) -> Result<model::Read<Reading>, CliError> {
        model::readings(test, &self.client.data(&client::tests_path(test.id()))?)
    }

    /// The readings of `tests`, one read each, in order.
    pub fn readings(&self, tests: &[Test]) -> Result<Readings, CliError> {
        let mut out = Readings::default();
        for &t in tests {
            let read = self.test(t)?;
            if read.unreadable > 0 {
                out.unreadable.insert(t, read.unreadable);
            }
            out.items.extend(read.items);
        }
        out.items.sort_by(|a, b| b.date.cmp(&a.date).then(a.test.cmp(&b.test)));
        Ok(out)
    }

    pub fn donations(&self) -> Result<model::Read<Donation>, CliError> {
        model::donations(&self.client.data(client::DONATIONS)?)
    }

    /// `oneblood api GET <path>`, the path already checked by
    /// [`client::api_path`].
    pub fn api(&self, path: &str) -> Result<Value, CliError> {
        self.client.api(path)
    }

    /// Write the cookies the answers changed back to the profile's session,
    /// only while the stored session is still the one this command read: a
    /// sign-in made meanwhile is never overwritten. Whether it wrote.
    pub fn finish(&self) -> Result<bool, CliError> {
        let Some(header) = self.client.updated_cookie_header() else {
            return Ok(false);
        };
        match self.store.get(&self.profile)? {
            Some(stored) if stored.cookie_header.expose() == self.session.cookie_header.expose() => {
                self.store
                    .put(&self.profile, &self.session.with_cookie_header(header))?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// [`Site::finish`] after a command's output is complete: a failed
    /// write-back is a warning, since the output stands and the stored
    /// session still works.
    pub fn finish_quietly(&self) {
        if let Err(e) = self.finish() {
            eprintln!(
                "warning: the session's refreshed cookies were not stored: {}",
                e.message()
            );
        }
    }
}

/// The stream header's organization and the patients list's `organization`.
pub fn organization() -> Organization {
    Organization {
        id: fhir::ORGANIZATION_ID.into(),
        name: fhir::ORGANIZATION_NAME.into(),
        fhir_base: fhir::SITE.into(),
        vendor: Some("oneblood".into()),
        location: Some("Florida and the Southeast, US".into()),
        aliases: Vec::new(),
    }
}

/// The donor as a patient context. `name` only for the donor's own
/// terminal (`patients list`), never in the stream.
pub fn context(d: &Donor, name: Option<String>) -> PatientContext {
    PatientContext {
        id: d.id.clone(),
        organization: fhir::ORGANIZATION_ID.into(),
        label: None,
        relationship: Some(relationship::SELF.into()),
        kind: Some(kind::PATIENT.into()),
        member_role: None,
        fhir_user: Some(format!("Patient/{}", d.id)),
        name,
        session: None,
    }
}

/// `patients list`: the one donor a login is (patient-list/v1).
pub fn patients(site: &Site) -> Result<Value, CliError> {
    let d = site.donor()?;
    let list = pk_med_records::patient_list(vec![context(&d, d.name())]);
    let mut v = serde_json::to_value(&list).map_err(|e| CliError::Other(format!("serializing: {e}")))?;
    if let Some(item) = v.pointer_mut("/items/0") {
        if let Some(bt) = &d.blood_type {
            item["blood_type"] = bt.clone().into();
        }
    }
    Ok(v)
}

/// The patient `--patient` names, which must be the profile's donor: a
/// login is one donor. Errors name the id given, never the donor's.
pub fn check_patient(d: &Donor, given: Option<&str>) -> Result<(), CliError> {
    match given.map(str::trim) {
        None | Some("") => Ok(()),
        Some(g) if g == d.id => Ok(()),
        Some(g) => Err(
            CliError::NotFound(format!("no patient `{g}` at this login: a OneBlood login is one donor"))
                .with_remedy(format!("`{BIN} patients list --profile <NAME>` shows its id")),
        ),
    }
}

pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    pk_cli_core::dates::fmt_rfc3339(secs)
}

/// The stream header for the donor.
pub fn header(d: &Donor, since: Option<&str>) -> Header {
    let mut h = Header::new(
        Source::new(BIN, env!("CARGO_PKG_VERSION")),
        organization(),
        context(d, None),
        now_rfc3339(),
    );
    h.method = Some(method::PORTAL.into());
    if let Some(s) = since {
        h.since = Some(s.into());
        // Written whatever the date: the donor, OneBlood, and the blood type,
        // which the portal gives no date for.
        h.since_unbounded = vec!["Organization".into(), "Patient".into()];
    }
    h
}

/// `records export`: the stream on `out`. Nothing reaches `out` before the
/// donor is read, so a refused session is an ordinary error; once the header
/// is out, a failure ends the stream with an incomplete trailer.
pub fn export<W: Write>(site: &Site, out: W, patient: Option<&str>, since: Option<&str>) -> Result<Finished, CliError> {
    let d = site.donor()?;
    check_patient(&d, patient)?;
    let mut w = RecordsWriter::start(out, &header(&d, since))?;
    let result = (|| -> Result<(), CliError> {
        w.resource(&RawResource::from_value(&fhir::organization())?)?;
        w.resource(&RawResource::from_value(&fhir::patient(&d))?)?;
        if let Some(bt) = fhir::blood_type(&d) {
            w.resource(&RawResource::from_value(&bt)?)?;
        }
        let read = site.readings(&Test::ALL)?;
        for (test, n) in &read.unreadable {
            w.skip(Skip::resource(
                "Observation",
                "",
                "unreadable",
                format!("{n} {} reading(s) came without a date or a value", test.name()),
            ));
        }
        let kept: Vec<Reading> = read
            .items
            .into_iter()
            .filter(|r| since.is_none_or(|s| r.date.as_str() >= s))
            .collect();
        let (resources, repeated) = fhir::readings(&d, &kept);
        if repeated > 0 {
            w.skip(Skip::resource(
                "Observation",
                "",
                "duplicate",
                format!("{repeated} reading(s) repeated a test already read for the same day"),
            ));
        }
        for r in &resources {
            w.resource(&RawResource::from_value(r)?)?;
        }
        Ok(())
    })();
    w.finish(now_rfc3339(), result.as_ref().err())
}

fn reading_json(r: &Reading) -> Value {
    let mut v = json!({
        "date": r.date,
        "test": r.test.name(),
        "value": r.value,
        "unit": r.test.unit_display(),
    });
    if r.is_number() {
        v["number"] = json!(true);
    }
    v
}

pub const READING_COLUMNS: &[&str] = &["date", "test", "value", "unit"];

fn in_range(date: &str, since: Option<&str>, until: Option<&str>) -> bool {
    since.is_none_or(|s| date >= s) && until.is_none_or(|u| date <= u)
}

fn unreadable_json(unreadable: &BTreeMap<Test, usize>) -> Value {
    Value::Object(
        unreadable
            .iter()
            .map(|(t, n)| (t.name().to_string(), json!(n)))
            .collect(),
    )
}

/// `readings list` (reading-list/v1): one item per reading, newest first.
pub fn readings_list(site: &Site, tests: &[Test], since: Option<&str>, until: Option<&str>) -> Result<Value, CliError> {
    let read = site.readings(tests)?;
    let items: Vec<Value> = read
        .items
        .iter()
        .filter(|r| in_range(&r.date, since, until))
        .map(reading_json)
        .collect();
    let mut v = json!({ "schema": "reading-list/v1", "items": items });
    if !read.unreadable.is_empty() {
        v["unreadable"] = unreadable_json(&read.unreadable);
    }
    Ok(v)
}

pub const DONATION_COLUMNS: &[&str] = &[
    "date",
    "procedure",
    "location",
    "cholesterol",
    "blood_pressure",
    "pulse",
    "temperature",
    "hemoglobin",
];

/// `donations list` (donation-list/v1): the donations, newest first, each
/// with the readings taken that day. A day with readings but no donation
/// listed (the site's history and its tests disagree) is an item too, with
/// no procedure, so no reading goes unshown.
pub fn donations_list(site: &Site, since: Option<&str>, until: Option<&str>) -> Result<Value, CliError> {
    let donations = site.donations()?;
    let read = site.readings(&Test::ALL)?;
    let today = &now_rfc3339()[..10];
    let mut by_day: BTreeMap<String, Vec<&Reading>> = BTreeMap::new();
    for r in &read.items {
        by_day.entry(r.date.clone()).or_default().push(r);
    }
    let mut items: Vec<Value> = Vec::new();
    let mut listed = std::collections::BTreeSet::new();
    let fill = |v: &mut Value, rs: Option<&Vec<&Reading>>| {
        let Some(rs) = rs else { return };
        let get = |t: Test| rs.iter().find(|r| r.test == t).map(|r| r.value.clone());
        for t in [Test::Cholesterol, Test::Pulse, Test::Temperature, Test::Hemoglobin] {
            if let Some(val) = get(t) {
                v[t.name()] = val.into();
            }
        }
        match (get(Test::Systolic), get(Test::Diastolic)) {
            (Some(s), Some(d)) => v["blood_pressure"] = format!("{s}/{d}").into(),
            (Some(s), None) => v["blood_pressure"] = format!("{s}/?").into(),
            (None, Some(d)) => v["blood_pressure"] = format!("?/{d}").into(),
            (None, None) => {}
        }
    };
    for d in &donations.items {
        listed.insert(d.date.clone());
        let mut v = json!({ "date": d.date });
        if let Some(p) = &d.procedure {
            v["procedure"] = p.clone().into();
        }
        if let Some(l) = &d.location {
            v["location"] = l.clone().into();
        }
        v["mobile_drive"] = d.center_id.is_none().into();
        if d.date.as_str() > today {
            v["upcoming"] = true.into();
        }
        fill(&mut v, by_day.get(&d.date));
        items.push(v);
    }
    for (day, rs) in &by_day {
        if !listed.contains(day) {
            let mut v = json!({ "date": day });
            fill(&mut v, Some(rs));
            items.push(v);
        }
    }
    items.retain(|v| in_range(v["date"].as_str().unwrap_or(""), since, until));
    items.sort_by(|a, b| b["date"].as_str().cmp(&a["date"].as_str()));
    let mut v = json!({ "schema": "donation-list/v1", "items": items });
    let mut u = unreadable_json(&read.unreadable);
    if donations.unreadable > 0 {
        u["donations"] = json!(donations.unreadable);
    }
    if u.as_object().is_some_and(|o| !o.is_empty()) {
        v["unreadable"] = u;
    }
    Ok(v)
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

    #[test]
    fn the_patient_must_be_the_donor() {
        check_patient(&donor(), None).unwrap();
        check_patient(&donor(), Some("D0001")).unwrap();
        let e = check_patient(&donor(), Some("X9")).unwrap_err();
        assert_eq!(e.exit_code(), 4);
        assert!(!e.message().contains("D0001"), "{e}");
    }

    #[test]
    fn the_context_names_no_one_unless_asked() {
        let c = context(&donor(), None);
        assert_eq!(c.id, "D0001");
        assert_eq!(c.organization, "oneblood");
        assert_eq!(c.name, None);
        let h = header(&donor(), Some("2025-01-01"));
        assert_eq!(h.method(), "portal");
        assert_eq!(h.patient.name, None);
        assert_eq!(h.since_unbounded, ["Organization", "Patient"]);
    }
}
