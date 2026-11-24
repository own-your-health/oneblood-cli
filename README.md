# oneblood

Unofficial; not affiliated with or endorsed by OneBlood, Inc.

`oneblood` is a read-only command line for a [OneBlood](https://www.oneblood.org)
blood donor's own health readings. OneBlood records each donor's blood type,
and at every donation their total cholesterol, blood pressure, pulse,
temperature and hemoglobin (the portal's "iron count"). `oneblood` reads
them from the donor portal you already sign in to, as you, and prints them
or writes them as FHIR R4 for a personal medical-records store
(`records-stream/v1`, the `health-records/v1` profile of
[medical-common](https://github.com/own-your-health/medical-common), read by
`medrec import --from oneblood`).

## Who it is for

Anyone with a OneBlood donor-portal account: you sign in at
oneblood.org with your email address and password. OneBlood collects in
Florida and parts of Georgia, Alabama and the Carolinas. A donor who has
not given blood yet has an account but no readings.

No developer registration is needed. OneBlood publishes no API and no
developer program, so there is no app to register and no key to request:
`oneblood` signs in to the same portal pages you use and reads the same
JSON those pages read (`docs/api.md`).

## Install

```sh
cargo install --git https://github.com/own-your-health/oneblood-cli --locked
# or download a release, oneblood-<target>.tar.gz (macOS Apple Silicon or
# Intel, Linux x86_64), put `oneblood` on your PATH; later: oneblood self-update
```

It needs Google Chrome for the sign-in (`ONEBLOOD_CHROME` names another
Chromium-based browser), and the macOS keychain or, on Linux, a Secret
Service keyring such as GNOME Keyring for the stored session.

## Quick start

```sh
oneblood profiles add me                          # one profile per donor-portal login
oneblood auth login --browser --profile me        # a Chrome window opens; sign in there
oneblood patients list --profile me               # your donor id and blood type
oneblood donations list --profile me              # each donation with its readings
oneblood readings list --profile me --test blood-pressure --since 2025-01-01
oneblood records export --profile me --ndjson > readings.ndjson
```

Into [medrec](https://github.com/own-your-health/medrec-cli), the records store:

```sh
medrec profiles add records
medrec --profile records init ~/MedicalRecords
medrec --profile records people add "<your name>" --birth-date <YYYY-MM-DD>
medrec --profile records import --from oneblood --from-profile me \
  --patient <donor id from `patients list`> --person "<your name>"
```

A medrec older than 0.2.7 does not list `oneblood` among the providers it
looks for: run `medrec config set providers oneblood` once first.

Every command that reads the portal takes `--profile <NAME>`
(`ONEBLOOD_PROFILE`); there is no default. `--json` on every command prints
one schema-tagged document.

## What needs a person

`auth login --browser` opens Chrome on OneBlood's sign-in page, in a browser
profile kept for that `oneblood` profile, and waits up to 10 minutes. On
OneBlood's own pages you:

1. enter your email address and password, optionally tick "Keep me signed
   in", and click Sign In;
2. choose where the one-time code goes, email or text, and click Send
   Code. OneBlood asks for a code at every sign-in, and its page says that
   choosing a method opts the account back into OneBlood's communications;
3. enter the code.

`oneblood` never sees your password or the code. Once the portal shows you
signed in, it reads the session's cookies, checks them with one read of your
account, stores them encrypted (`SECURITY.md`) and closes the window.

How long a sign-in lasts has not been measured yet. The portal's session
cookie carries no expiry date, so it ends when OneBlood ends it. When it has,
a read exits 3 and prints the command that signs in again:
`oneblood auth login --browser --profile <NAME> --overwrite`.

## Commands

| Command | Output |
|---|---|
| `profiles list\|add\|remove` | `profile-list/v1` |
| `auth login --browser`, `auth status\|logout\|reset` | `auth-login/v1`, `auth-status/v1`, … |
| `patients list` | `patient-list/v1`: the donor, with blood type |
| `records export --ndjson` | `records-stream/v1` (`docs/fhir.md`) |
| `donations list` | `donation-list/v1`: each donation with that day's readings |
| `readings list [--test T]` | `reading-list/v1`; `T` is `cholesterol`, `blood-pressure`, `pulse`, `temperature` or `hemoglobin` (`iron`) |
| `api GET <PATH>` | the portal's answer, as it came (read paths only) |
| `info` | `cli-info/v1` |

Exit codes: 0 ok, 2 usage, 3 signed out (sign in again), 4 not found,
5 OneBlood or the network failed, 6 confirmation needed (`--force`).

## OneBlood's terms and automated access

OneBlood's website Terms & Conditions (oneblood.org/terms-and-conditions.html,
last updated November 2023) do not mention scripts, bots, scraping or
automated access either way. They say you may not reach parts of the site
you are not authorized for, that the terms can change at any time without
notice, and that you use the site at your own risk. The site's `robots.txt`
asks crawlers to stay out of `/donor-portal/`; `oneblood` is not a crawler,
but it does read your own account's data behind that sign-in with a
program rather than a browser. It reads only what your own sign-in shows
you, one request at a time and spaced apart, never writes anything, and
never signs in for you. Whether that suits you under OneBlood's terms is
your call; OneBlood could change its terms or its portal at any time.

## Privacy

Readings stay on your machine: `oneblood` prints them or pipes them to your
own records store, and contacts no server but oneblood.org (and GitHub for
`self-update`). The repository holds no personal data; its fixtures are
synthetic.

## License

Copyright (C) 2026 piekstra. Licensed under the GNU Affero General Public License, version 3 only (AGPL-3.0-only); see LICENSE. Releases up to and including v0.1.1 were published under MIT OR Apache-2.0.
