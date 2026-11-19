# Changelog

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
