# Test fixtures

Every file here is **synthetic**. No value in them is a real donor's, and no
real answer was copied to write them.

## site/

The donor portal's answers to oneblood's reads (`docs/api.md`), one per
answer. Their structure follows the real answers: the same envelope
(`status`, `data`), keys, nesting and types. Every value is invented, in the
dummy shapes below.

| File | The answer to |
|---|---|
| `user.json` | `GET /api/invita/mobileuser/resource`: the donor's account |
| `tests-<id>.json` | `GET /api/invita/mobileuser/resource/?tests=<id>`: 1 cholesterol, 3 systolic, 4 diastolic, 5 pulse, 6 temperature, 9 hemoglobin |
| `donations.json` | `GET /api/invita/mobileuser/resource/?donations=1` |
| `signed-out.json` | a read without a session |

Dummy shapes, enforced by `tests/no_personal_data.rs`:

| Field | Shape |
|---|---|
| `donor_id` | `SYND` + 6 digits |
| `first_name`, `last_name` | `Alex`, `Sam`, `Jordan`; `Example` |
| `username`, `email` | `@example.com` |
| `number` (phones) | `555-01xx` |
| `address`, `location` | names with `Example` or `Exampleton`; ZIP codes all zeros |

The readings are plausible numbers, not anyone's.
