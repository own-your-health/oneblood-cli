# FHIR mapping

`oneblood records export` maps what the donor portal shows into FHIR R4,
by hand (`src/fhir.rs`). The stream header's `method` is `portal`; every
resource carries `meta.source` `oneblood:portal` and the tag
`mapped-from-donor-portal`
(`https://github.com/piekstra/oneblood-cli/fhir/tag`).

## Resources

| Resource | id | From |
|---|---|---|
| `Organization` | `oneblood` | fixed: OneBlood, its site |
| `Patient` | the donor id | the account: `donor_id` as an identifier (system `https://github.com/piekstra/oneblood-cli/fhir/donor-id`, assigned by OneBlood), the first and last name |
| `Observation` | `blood-type` | the account's `blood_type` |
| `Observation` | `<test>-<YYYY-MM-DD>` | one reading, dated by its donation |

The ids are stable across runs: a test and a day. The portal holds one
reading per test per donation; a second one for the same test and day
would repeat an id, so it is left out and counted as a `duplicate` skip.

## Observations

| Reading | Portal test id | LOINC | Category | Unit (UCUM) |
|---|---|---|---|---|
| Blood type | (account) | 882-1 ABO and Rh group [Type] in Blood | laboratory | value is a SNOMED CT blood group |
| Total cholesterol | 1 | 2093-3 Cholesterol [Mass/volume] in Serum or Plasma | laboratory | `mg/dL` |
| Blood pressure | 3 and 4 | 85354-9 Blood pressure panel, components 8480-6 systolic and 8462-4 diastolic | vital-signs | `mm[Hg]` |
| Pulse | 5 | 8867-4 Heart rate | vital-signs | `/min` |
| Temperature | 6 | 8310-5 Body temperature | vital-signs | `[degF]` |
| Hemoglobin ("iron count") | 9 | 718-7 Hemoglobin [Mass/volume] in Blood | laboratory | `g/dL` |

- Systolic and diastolic of one day are one panel. When the portal has only
  one of them for a day, the other component carries
  `dataAbsentReason: unknown`.
- A value the portal serves as a plain decimal is a `valueQuantity` (an
  integer stays an integer); anything else is the portal's text as
  `valueString`.
- The portal calls hemoglobin its "iron count" and charts it in g/dL, the
  hemoglobin screen every donor gets before donating.
- The blood type's value keeps the portal's words as `text` beside the
  SNOMED CT code: `A Positive` is 278149003, `A Negative` 278152006,
  `B Positive` 278150003, `B Negative` 278153001, `AB Positive` 278151004,
  `AB Negative` 278154007, `O Positive` 278147001, `O Negative` 278148006.
  A type the mapping does not know keeps only the text. The portal gives no
  date for it, so the Observation carries none, and `--since` always keeps
  it.
- `performer` is OneBlood; `subject` the donor's `Patient`.
