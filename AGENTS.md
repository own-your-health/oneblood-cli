# AGENTS.md — oneblood-cli

`oneblood` is a read-only CLI for a OneBlood blood donor's blood type and
the readings OneBlood takes at each donation: total cholesterol, blood
pressure, pulse, temperature and hemoglobin (the portal's "iron count"). It
reads the donor portal on www.oneblood.org, signed in per profile, and
emits FHIR for the household's medical-records system (`records-stream/v1`,
`medrec import --from oneblood`). It conforms to piekstra-cli/1 and the
`health-records/v1` profile, on the `pk-cli-*` crates from
[cli-common](https://github.com/piekstra/cli-common) and `pk-med-records`
from [medical-common](https://github.com/piekstra/medical-common). Every
`pk-cli-*` stays on the cli-common tag medical-common pins.

## Layout

| File | Owns |
|---|---|
| `src/cli.rs` | the clap tree |
| `src/app.rs` | dispatch, argument checks, profiles, config, info, output |
| `src/auth.rs` | `auth login`, `status`, `logout` of one `--profile`, `auth reset`: the browser-profile seams |
| `src/session.rs` | `SessionStore`: get, put and delete a profile's session in the vault; the only code that knows where it lives |
| `src/site.rs` | the sign-in: the browser capture and the session check (`SESSION_CHECK_URL`, `judge`) |
| `src/client.rs` | the reads: the paths, pacing, the cookie jar, `judge` (answers to data or exit codes), and `api_path`, which checks an `api` path is a read before it is sent |
| `src/model.rs` | the answers into oneblood's types: `Donor`, `Reading`, `Donation`, `Test` |
| `src/fhir.rs` | the FHIR mapping (`docs/fhir.md`) |
| `src/portal.rs` | the signed-in commands (`Env`: where sessions are kept and which site is read; `Site`, which writes refreshed cookies back) |
| `docs/api.md` | the portal's endpoints as observed, and the traps |

## Medical data rules

These come from the household's plan and are not negotiable.

- The repository carries no personal data, in code, fixtures, docs, commits,
  pull requests or issues. Fixtures are synthetic (`tests/fixtures/README.md`);
  `tests/no_personal_data.rs` scans every tracked file and checks every
  identity field in the fixtures against its dummy shape.
- Records and their contents are never sent to an outside service: no
  health-data aggregator, cloud OCR or other company's API. A Claude session
  is not one of these (the household's decision, 2026-11-17). A session may
  read, print and summarize the household's real readings when the work
  needs it: debugging a real export, mapping the portal with real answers,
  checking a pull. Real data stays out of tracked files, commits, pull
  requests and issues. A raw capture is deleted from `/tmp` once it has
  served.
- `records export -o DIR` writes a plain file, so it reads only a test's
  loopback server; real records go straight into medrec through a pipe.
- No FHIR or SMART libraries, no aggregators. The FHIR here is mapped by
  hand in `src/fhir.rs`; general-purpose crates are fine.
- Read-only. Nothing writes to OneBlood: every request is a `GET` of the
  portal's own JSON. A write would need SPEC §1.3's confirmation gate and a
  change to this paragraph in the same commit.

## Profiles and the session

Every command that reads the portal takes `--profile <NAME>`
(`ONEBLOOD_PROFILE`), which must be configured (`profiles add`); a missing
or unknown one is exit 2 listing the configured ones, and a lone profile is
never picked (`pk_cli_config::profile::resolve`). A session is the vault
entry `<profile>/session` (`pk_cli_auth::vault::Vault::for_binary("oneblood")`:
one keychain item, `vault-key` in `piekstra.oneblood`, and an encrypted file
per entry under `~/.config/oneblood/vault/`). A `--browser` sign-in runs in
the profile's own kept Chrome profile (`Launch::profile_name`).
`profiles remove` removes the session and that browser profile before the
profile; `auth reset` is `Vault::clear`.

The sign-in is OneBlood's own page: email address, password, then a
one-time code OneBlood sends by email or text. The window `auth login
--browser` opens is a plain Chrome with `--remote-debugging-port=0`, so a
session filling the sign-in for the owner drives it through the port in
`DevToolsActivePort` in the profile directory (the `browser-explore`
harness, `--port`). The capture waits for the `userLoggedIn` cookie, then
confirms the jar with one read of the donor's account before storing it.

`client::Client` sends one `GET` at a time, at least `client::PACE` apart,
follows no redirect, and maps answers to exit codes: a 401, a redirect or
the portal's own "not signed in" answer is 3 (`auth login --browser
--profile <NAME> --overwrite`); a 403 page, 429, 5xx and an error answer are
5. A `Set-Cookie` updates the jar, written back once the command has read
what it needed, unless a newer sign-in replaced the stored session
meanwhile (`portal::Site::finish`). A failed write-back is a warning.

## Tests are offline, always

`cargo test` reads no real keychain item or browser profile, stores no
session in the real vault, and contacts nothing beyond loopback. Every test
that runs a credential-reading command sets
`ONEBLOOD_KEYCHAIN_SERVICE=piekstra.oneblood.hermetic-tests`, a service that
holds nothing, with a temp `XDG_CONFIG_HOME`.

- `tests/cli_surface.rs`: every `--help`, exit codes, JSON errors, argument
  checks before a session is read, profile selection, `NO_COLOR`.
- `tests/site_e2e.rs`: every portal command in-process against a loopback
  server answering with `tests/fixtures/site/`, the session in an in-memory
  vault: the requests, the DTOs, the stream, write-back, and the refusals.
- `tests/no_personal_data.rs`.

`make verify` = fmt-check, clippy, test, smoke.

## Dogfooding on synthetic data

`ONEBLOOD_BASE_URL` points every read at a test's server instead of
www.oneblood.org. It is honored only for a plain `http` origin on a
loopback address, and only with `ONEBLOOD_KEYCHAIN_SERVICE` set, so no
session the owner stored can be sent to it (`client::base_from`);
`auth login --browser` refuses it. `scripts/fixture-site.py` serves
`tests/fixtures/site/` that way, with `--status PATH=CODE` for the error
paths and `--set-cookie` for the write-back; its docstring has the steps.
A dogfood run stores a vault key under its test keychain service:
`auth reset --force` under the same variables removes it.
