# oneblood

An unofficial, read-only command line for a [OneBlood](https://www.oneblood.org)
blood donor's own health readings: blood type, and the total cholesterol,
blood pressure, pulse, temperature and hemoglobin OneBlood records at each
donation. It reads the donor portal you already use, signed in as you, and
writes FHIR for a personal medical-records store (`records-stream/v1`, the
`health-records/v1` profile of [medical-common](https://github.com/piekstra/medical-common)).

OneBlood publishes no API; oneblood reads the same JSON the portal's own
pages read (`docs/api.md`). Unofficial; not affiliated with or endorsed by OneBlood, Inc.

## Install

```sh
oneblood self-update            # once installed
make install                    # from a checkout (Rust 1.85+)
```

Releases carry `oneblood-<target>.tar.gz` for macOS (Apple Silicon, Intel)
and Linux x86_64.

## Use

```sh
oneblood profiles add me                         # one profile per donor-portal login
oneblood auth login --browser --profile me       # sign in in a Chrome window (password, emailed code)
oneblood patients list --profile me              # the donor: id, blood type
oneblood readings list --profile me --test blood-pressure
oneblood donations list --profile me --since 2025-01-01
oneblood records export --profile me --ndjson | medrec import --from oneblood --from-profile me --patient <ID>
```

Every command that reads the portal takes `--profile <NAME>`
(`ONEBLOOD_PROFILE`); there is no default. `--json` on every command prints
one schema-tagged document.

| Command | Output |
|---|---|
| `profiles list\|add\|remove` | `profile-list/v1` |
| `auth login --browser`, `auth status\|logout\|reset` | `auth-login/v1`, `auth-status/v1`, … |
| `patients list` | `patient-list/v1` |
| `records export --ndjson` | `records-stream/v1` (`docs/fhir.md`) |
| `donations list` | `donation-list/v1`: each donation with its readings |
| `readings list [--test T]` | `reading-list/v1` |
| `api GET <PATH>` | the portal's answer, as it came (read paths only) |
| `info` | `cli-info/v1` |

Exit codes: 0 ok, 2 usage, 3 signed out (sign in again), 4 not found,
5 OneBlood or the network failed.

## Sign-in

`auth login --browser` opens Chrome on OneBlood's sign-in page in a browser
profile kept for that oneblood profile. You sign in on OneBlood's page:
email address, password, then the code OneBlood sends. oneblood never sees
the password. Once the portal shows you signed in, oneblood reads the
session's cookies, checks them with one read of your account, stores them
encrypted (`SECURITY.md`) and closes the window. When the session expires, a
read exits 3 and names the command that signs in again.

## Privacy

Readings stay on your machine: oneblood prints them or pipes them to your
own records store. The repository holds no personal data; its fixtures are
synthetic.

## License

MIT or Apache-2.0.
