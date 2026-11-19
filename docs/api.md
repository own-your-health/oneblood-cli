# The OneBlood donor portal

OneBlood publishes no API and no developer program. The donor portal is
pages on www.oneblood.org (an Adobe Experience Manager site) whose scripts
read same-origin JSON with a cookie session. oneblood reads that JSON, as
the pages do, with `GET`s only.

Status: the paths and answer fields below were read from the site's public
pages and its script bundle (`clientlib-site`), without signing in. Each is
confirmed against the signed-in portal's own traffic before a release; a
line marked *bundle* is not yet.

## Where the portal is

- `https://donorportal.oneblood.org/` answers `301` to
  `https://www.oneblood.org/donor-portal.html`.
- Sign-in: `https://www.oneblood.org/sign-in.html`. Signed-in pages are
  under `/donor-portal/` (account settings, health summary, history).

## Sign-in (bundle)

The sign-in page's script, not its `<form>`, does the work:

1. `POST /api/invita/session/generate_login_token?login_method=username`
   (empty form): a login token and a `notBeforeDelay` to wait before using
   it.
2. `POST /api/invita/session/login`, form fields `login_token`, `username`,
   `password`: on success `data.mfa_token` and `data.factors` (the email
   addresses and phone numbers a code can go to). An answer with
   `error_message: "invalid_login_token"` is retried after 2 s.
3. The page moves to the code step, carrying `mfa_token`, `username` and
   `factors` in its query.
4. `POST /api/invita/session/send_otp`, fields `username`, `mfa_token`,
   `factor_id`: sends the code.
5. `POST /content/oneblood/us/en/jcr:content.invita.json?type=POST_LOGIN_OTP`,
   fields `mfa_token`, `otp`: completes the sign-in and sets the session.

Every sign-in asks for a code. Sign-out is
`POST /content/oneblood/us/en/jcr:content.invita.json?type=POST_LOGOUT`.
oneblood sends none of these: the donor signs in on the page
(`auth login --browser`), and `oneblood api` refuses `/api/invita/session/`.

The pages tell a signed-in visitor by the cookie `userLoggedIn=true`; the
"keep me signed in" box sets `oblKeepSignedIn`.

## Reads

All under `/api/invita/mobileuser/resource`, answering
`{"status": "success", "data": …}`, or `{"status": "error", …}` with
`metadata.status_code` and `error_message`.

| Read | Path | `data` (bundle) |
|---|---|---|
| The account | `/api/invita/mobileuser/resource` | `donor_id` (a prospect who has not donated has `id` only), `user_type`, `first_name`, `last_name`, `username`, `blood_type` (`A Positive` … `O Negative`), `address`, `emails[].email`, `phones[]` |
| A test's history | `…/resource/?tests=<id>` | `tests[]` of `{test_date: "YYYY-MM-DD", value}` |
| Donations | `…/resource/?donations=1` | a list (or `donations[]`) of `{date, procedure, location, center_id}`; `center_id` `"0"` is a mobile drive |
| Appointments | `…/resource/?appointments=1` | booked appointments |
| Blood type alone | `…/resource/?blood_type=1` | `blood_type` |
| Gallons given | `…/resource/?gallon_level=1` | `gallon_level` |
| Eligibility | `…/resource/?eligibility=1` | next eligible dates per procedure |

Test ids, as the health summary's charts use them:

| id | Test | Unit the chart shows |
|---|---|---|
| 1 | Cholesterol | mg/dL |
| 3 | Blood pressure, systolic | mmHg |
| 4 | Blood pressure, diastolic | mmHg |
| 5 | Pulse | beats per minute |
| 6 | Temperature | Fahrenheit |
| 9 | "Iron count" (hemoglobin) | g/dL |

The backend's name, "invita", is in every path; who makes it is not stated
anywhere public.

## Writes oneblood never sends

Appointments are booked and cancelled with `POST`s to the same resources
(`?create_appointment_center`, `?create_appointment_drive`,
`?cancel_appointment`, with a token). `oneblood api` refuses any query
naming an action, and any method but `GET`.

## Traps

- The sign-in `<form>` has no action; the page's script sends the requests
  above. Copying the form would send nothing the site accepts.
- The site sets visitor cookies before anyone signs in, so a cookie in the
  jar proves nothing; oneblood confirms a session with one read of the
  account.
