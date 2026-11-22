# Changelog

## 0.1.1

- The repository moved to own-your-health/oneblood-cli. `self-update`
  and `info` name the new owner, and medical-common is fetched from
  own-your-health/medical-common (same v0.6.2 tag).
- The FHIR identifier systems are unchanged
  (`https://github.com/piekstra/oneblood-cli/fhir/tag` and `…/fhir/donor-id`):
  records already in medrec carry them, and medrec matches a Patient by its
  identifier's system and value.

## 0.1.0

- `oneblood`: a OneBlood donor's blood type and per-donation readings
  (cholesterol, blood pressure, pulse, temperature, hemoglobin) from the
  donor portal, read-only.
- `profiles list|add|remove`; every portal command takes `--profile`.
- `auth login --browser` signs in through a Chrome window in the profile's
  own browser profile; `auth status|logout|reset`.
- `patients list` (patient-list/v1), `records export --ndjson`
  (records-stream/v1, for `medrec import --from oneblood`),
  `donations list` (donation-list/v1), `readings list` (reading-list/v1),
  `api GET <PATH>`, `config path|show`, `info`, `completions`,
  `self-update`.
