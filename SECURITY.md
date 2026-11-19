# Security

Report a vulnerability privately through GitHub's "Report a vulnerability"
(Security tab), not in an issue.

## What oneblood touches

- Contacts no server except GitHub, for `self-update`, and
  `https://www.oneblood.org`: for `auth login`, a Chrome window the donor
  signs in through and one read that checks the session before it is
  stored; for a command run with `--profile`, the donor's own reads. Every
  request is a `GET` of the portal's own JSON under
  `/api/invita/mobileuser/resource`, one at a time and spaced apart, with a
  30-second limit and no redirect followed. Every URL is built from that
  fixed origin. `oneblood api` is held to the same: a `GET` of a portal data
  path, refused before anything is sent when it names the sign-in paths or
  an action.
- A cookie the site changes is written back to the profile's session once
  the command has read what it needed, unless a newer sign-in replaced the
  stored session meanwhile. A failed write-back is a warning on stderr.
- Stores one credential per profile, its portal session: the `Cookie`
  header, the browser's user agent and the capture time, as the vault entry
  `<profile>/session`. The vault (`pk_cli_auth::vault`) is encrypted
  owner-only files under `~/.config/oneblood/vault/`, with one key in the
  keychain (service `piekstra.oneblood`, account `vault-key`). A session
  never goes on a command line or into a log.
- Keeps one Chrome profile per oneblood profile for `auth login --browser`:
  `~/Library/Application Support/piekstra.oneblood/browser-profile-<profile>`
  (under `$XDG_DATA_HOME` on Linux), owner-only (`0700`; a launch is refused
  otherwise). `--fresh-profile` signs in without it.
- `auth logout --profile <NAME>` and `profiles remove <NAME>` remove that
  profile's session, then its Chrome profile. `auth reset` removes every
  session and the vault key.
- Writes a file only for `records export -o`, and only against a test's
  loopback server: real records stream to stdout.
- The config file holds the profile names.

## What oneblood never does

- Writes to OneBlood: no appointment, account or sign-out request.
- Signs in by itself: the donor's password and code are typed (or filled)
  on OneBlood's own page, never passed to oneblood.
- Sends readings anywhere: the output goes to stdout or a file you name.
