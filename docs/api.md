# The OneBlood donor portal

OneBlood publishes no API and no developer program. The donor portal is
pages on www.oneblood.org (an Adobe Experience Manager site) whose scripts
read same-origin JSON with a cookie session. oneblood reads that JSON, as
the pages do, with `GET`s only.

Status: sign-in steps 1 to 3 were observed on the portal on 2026-11-19,
and the four reads oneblood makes (the account, a test's history,
donations) on 2026-11-20, signed in, through `oneblood api`. Lines marked
*bundle* were read from the site's script bundle (`clientlib-site`) and
not seen on the wire: the requests behind sign-in steps 4 and 5, and the
reads oneblood does not make.

## Where the portal is

- `https://donorportal.oneblood.org/` answers `301` to
  `https://www.oneblood.org/donor-portal.html`.
- Sign-in: `https://www.oneblood.org/sign-in.html`. Signed-in pages are
  under `/donor-portal/` (account settings, health summary, history).

## Sign-in

The sign-in page's script, not its `<form>`, does the work. Steps 1 to 3
were observed on 2026-11-19; 4 and 5 are from the bundle.

1. `POST /api/invita/session/generate_login_token?login_method=username`,
   an empty `multipart/form-data` body: `200` JSON with a login token and a
   `notBeforeDelay` to wait before using it. It sets the load balancer's
   `AWSALB` and `AWSALBCORS` cookies (7 days).
2. `POST /api/invita/session/login`, fields `login_token`, `username`,
   `password`: `202` JSON with `data.mfa_token` and `data.factors`, and a
   `session` cookie with no expiry (it ends with the browser). An answer
   with `error_message: "invalid_login_token"` is retried after 2 s.
3. The page moves to `/sign-in/verify.html`, carrying `mfa_token`,
   `username` and `factors` in its query. `factors` is a JSON list, one
   entry per place a code can go: `factor_id` (`<type>_<medium id>`),
   `medium_id`, `factor_type` (`Email` or `SMS`), the address or phone
   number, and `confirmed`. The page asks to pick one (radio buttons
   `input[name=verification]`, valued with the `factor_id`; the Email one
   is hidden behind its `label`, so click the label), then "Send Code"
   (`button.cmp-button`).
4. The code arrives within a minute (the page says it may take 2 to 3),
   by email from OneBlood's `notify.oneblood.org` address with the subject
   "Verification Code", valid 10 minutes. *bundle:* the request is
   `POST /api/invita/session/send_otp`, fields `username`, `mfa_token`,
   `factor_id`.
5. The code goes in `#otp-pass` and the page's "Sign In" button is
   `#verify-button`. *bundle:* the request is
   `POST /content/oneblood/us/en/jcr:content.invita.json?type=POST_LOGIN_OTP`,
   fields `mfa_token`, `otp`, which completes the sign-in and sets the
   session, `userLoggedIn` among it.

Every sign-in asks for a code. Sign-out is
`POST /content/oneblood/us/en/jcr:content.invita.json?type=POST_LOGOUT`.
oneblood sends none of these: the donor signs in on the page
(`auth login --browser`), and `oneblood api` refuses `/api/invita/session/`.

The pages tell a signed-in visitor by the cookie `userLoggedIn=true`; the
"keep me signed in" box sets `oblKeepSignedIn`.

## Reads

All `GET`s under `/api/invita/mobileuser/resource`. A signed-in answer is
`200` JSON in one envelope: `status` (`"success"`), `response_timestamp`
(an HTTP date), `data`, `rows` (how many items, or keys, `data` holds) and
`token` (a long opaque string; oneblood ignores it and never prints it).
A visitor's answer is `{"status": "error", …}` with `metadata.status_code`
and `error_message` (*bundle*). Every value in `data` is a string except
booleans and a few integers, and ids are digit strings.

| Read | Path | `data`, as observed |
|---|---|---|
| The account | `/api/invita/mobileuser/resource` | `id`, `donor_id`, `user_type` (`donor`), `username`, `first_name`, `last_name`, `blood_type` (`A Positive` … `O Negative`), `birth_date`, `gender`, `address` (one line), `emails[]` (`id`, `email`, `primary`), `phones[]` (`id`, `number`, `primary`, `extension`, `allow_texting`, `phone_type_id`, `phone_type_name`, `phone_time_id`), `phone_types[]`, `avatar` (`image_data`, base64, `image_mime`), `linked_accounts[]`, `gallon_level`, `eligibility[]` (`name`, `date`, `sort`: the next date for each procedure), `lifetime_donations`, `reset_password`, `pending_deletion`, `coverage_date`, `profile_type_ids[]` |
| A test's history | `…/resource/?tests=<id>` | `tests[]`, each `id`, `becs_id`, `test_type_id`, `test_type_name`, `value`, `value_alt`, `test_date` (`YYYY-MM-DD`), `unit_number` (the blood unit's number), `created` (`YYYY-MM-DD HH:MM:SS`) |
| Donations | `…/resource/?donations=1` | `donations[]`, each `date`, `id`, `center_id` (`"0"` with `location_type_text` `Bloodmobile`: a mobile drive), `procedure` (free text, such as `2 Units RBC`), `donation_type`, `donation_status`, `location`, `location_id`, `location_type_text`, `unit_number`, `sponsor_code`, `concierge`, `survey_taken`, `survey_available`, `points_earned`, `optimum_donation`, `donation_category` |
| Appointments | `…/resource/?appointments=1` | *bundle:* booked appointments |
| Blood type alone | `…/resource/?blood_type=1` | *bundle:* `blood_type` |
| Gallons given | `…/resource/?gallon_level=1` | *bundle:* `gallon_level` |
| Eligibility | `…/resource/?eligibility=1` | *bundle:* next eligible dates per procedure |

oneblood reads, of these: `donor_id`, `first_name`, `last_name` and
`blood_type` from the account; `test_date` and `value` from each test;
`date`, `procedure`, `location` and `center_id` from each donation. It
never reads the contact details, the avatar, the unit numbers or the
token, and never prints them.

Test ids, with the portal's `test_type_name` for each (observed) and the
unit its health-summary chart shows:

| id | `test_type_name` | Unit the chart shows |
|---|---|---|
| 1 | `Cholesterol` | mg/dL |
| 3 | `Blood Pressure (Systolic)` | mmHg |
| 4 | `Blood Pressure (Diastolic)` | mmHg |
| 5 | `Pulse` | beats per minute |
| 6 | `Temperature` | Fahrenheit |
| 9 | `Iron` (the hemoglobin screen, "iron count" on the chart) | g/dL |

The backend's name, "invita", is in every path; who makes it is not stated
anywhere public.

## Writes oneblood never sends

Appointments are booked and cancelled with `POST`s to the same resources
(`?create_appointment_center`, `?create_appointment_drive`,
`?cancel_appointment`, with a token). `oneblood api` refuses any query
naming an action, and any method but `GET`.

## Traps

- Picking where the code goes is also a consent: the code page says that
  choosing a method opts the account back into OneBlood's communications.
  That click is the donor's, never a script's.
- The Sign In control is an `<a class="cmp-button">`, not a submit button,
  and there is no `<form>` around the fields.

- The sign-in `<form>` has no action; the page's script sends the requests
  above. Copying the form would send nothing the site accepts.
- The site sets visitor cookies before anyone signs in, so a cookie in the
  jar proves nothing; oneblood confirms a session with one read of the
  account.
