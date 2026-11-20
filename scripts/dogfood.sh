#!/bin/bash
# Drives a oneblood binary through every command and error path against
# scripts/fixture-site.py, on the synthetic fixtures, and prints one row per
# case. Exits 1 if any case fails.
#
#     scripts/dogfood.sh                  # the oneblood on PATH (the installed one)
#     scripts/dogfood.sh target/release/oneblood
#
# It runs in a throwaway test store (AGENTS.md, "Dogfooding on synthetic
# data"): a vault key under the keychain service piekstra.oneblood.dogfood,
# created by the binary under test, so no keychain dialog, and removed at
# the end with `auth reset --force`. It never reads the owner's sessions.
# Ports DOGFOOD_PORT (default 8931) to DOGFOOD_PORT+2 must be free.
set -u

OB=${1:-$(command -v oneblood || true)}
[ -x "$OB" ] || { echo "usage: $0 [path to oneblood]" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
T=$(mktemp -d)
export ONEBLOOD_KEYCHAIN_SERVICE=piekstra.oneblood.dogfood
export XDG_CONFIG_HOME=$T/config XDG_DATA_HOME=$T/data
unset ONEBLOOD_PROFILE ONEBLOOD_BASE_URL

P=${DOGFOOD_PORT:-8931}  # the fixtures
Q=$((P + 1))             # 503 on one test, 429 on donations
R=$((P + 2))             # 401 on the account, 302 on one test
DOWN=$((P + 8))          # nothing listens
USER_PATH=/api/invita/mobileuser/resource
SITE="python3 -I $HERE/fixture-site.py"
$SITE "$P" 2>/dev/null & S1=$!
$SITE "$Q" --status "$USER_PATH/?tests=5=503" --status "$USER_PATH/?donations=1=429" 2>/dev/null & S2=$!
$SITE "$R" --status "$USER_PATH=401" --status "$USER_PATH/?tests=1=302" 2>/dev/null & S3=$!

cleanup() {
    "$OB" auth reset --force >/dev/null 2>&1
    kill "$S1" "$S2" "$S3" 2>/dev/null
    wait 2>/dev/null
    rm -rf "$T"
}
trap cleanup EXIT

for port in "$P" "$Q" "$R"; do
    for _ in $(seq 50); do
        curl -s -o /dev/null "http://127.0.0.1:$port/" && break
        sleep 0.1
    done
done

fails=0
check() { # NAME WANT GOT
    local r=ok
    if [ "$2" != "$3" ]; then r=FAIL; fails=$((fails + 1)); fi
    printf '| %s | %s | %s | %s |\n' "$1" "$2" "$3" "$r"
}
# The exit code of a command, stdin closed (so a prompt sees no terminal).
code() { # NAME WANT CMD...
    local name=$1 want=$2
    shift 2
    "$@" </dev/null >"$T/out" 2>"$T/err"
    check "$name" "exit $want" "exit $?"
}
# A Python expression over the command's JSON (`d`: the document, or a
# stream's last line, its trailer).
value() { # NAME WANT EXPR CMD...
    local name=$1 want=$2 expr=$3 got
    shift 3
    "$@" </dev/null >"$T/out" 2>"$T/err"
    got=$(python3 -I -c "
import json, sys
t = open(sys.argv[1]).read()
try:
    d = json.loads(t)
except ValueError:
    d = json.loads(t.strip().splitlines()[-1])
v = ($expr)
print(*v) if isinstance(v, tuple) else print(v)" "$T/out" 2>&1)
    check "$name" "$want" "$got"
}
at() { # PORT CMD...: read the fixture site on PORT
    local port=$1
    shift
    ONEBLOOD_BASE_URL=http://127.0.0.1:$port "$@"
}
login() { # COOKIE [FLAGS...]
    printf '%s' "$1" | "$OB" auth login --stdin --user-agent test --profile synth "${@:2}"
}
SESSION='dummy_session=1; userLoggedIn=true'
OUT=$T/export

echo "oneblood: $OB ($("$OB" --version))"
echo
echo '| Case | Want | Got | |'
echo '|---|---|---|---|'
value "info" "cli-info/v1" "d['schema']" "$OB" info --json
code "NO_COLOR=1" 0 env NO_COLOR=1 "$OB" info
code "no profile" 2 at "$P" "$OB" patients list
code "profiles add" 0 "$OB" profiles add synth
code "unknown profile" 2 at "$P" "$OB" patients list --profile nobody
code "no session stored" 3 at "$P" "$OB" patients list --profile synth
code "login --browser with the override" 2 at "$P" "$OB" auth login --browser --profile synth
code "login --stdin" 0 at "$P" login "$SESSION"
code "login again without --overwrite" 2 at "$P" login "$SESSION"
code "login with a signed-out cookie" 3 at "$P" login 'other=1' --overwrite
value "auth status" "True" "d['authenticated']" "$OB" auth status --profile synth --json
value "patients list" "1 O Positive" "len(d['items']), d['items'][0]['blood_type']" at "$P" "$OB" patients list --profile synth --json
value "readings list" "18" "len(d['items'])" at "$P" "$OB" readings list --profile synth --json
value "readings --test iron" "3" "len(d['items'])" at "$P" "$OB" readings list --profile synth --test iron --json
value "readings --test blood-pressure --since" "4" "len(d['items'])" at "$P" "$OB" readings list --profile synth --test blood-pressure --since 2025-01-01 --json
value "donations list: count, mobile drives" "3 1" "len(d['items']), sum(1 for i in d['items'] if i['mobile_drive'])" at "$P" "$OB" donations list --profile synth --json
value "records export" "True 16" "d['complete'], d['counts']['Observation']" at "$P" "$OB" records export --profile synth --ndjson
value "records export --since" "True 11" "d['complete'], d['counts']['Observation']" at "$P" "$OB" records export --profile synth --since 2025-03-01 --ndjson
code "records export -o" 0 at "$P" "$OB" records export --profile synth -o "$OUT"
check "records export -o: lines" "20" "$(wc -l <"$OUT/records.ndjson" | tr -d ' ')"
code "another donor's --patient" 4 at "$P" "$OB" records export --profile synth --patient SYND999999 --ndjson
code "bad --since" 2 at "$P" "$OB" records export --profile synth --since 2025-13-01 --ndjson
code "--ndjson with --json" 2 at "$P" "$OB" records export --profile synth --ndjson --json
value "api: the account" "success" "d['status']" at "$P" "$OB" api GET "$USER_PATH" --profile synth
code "api: a sign-in path" 2 at "$P" "$OB" api GET /api/invita/session/login --profile synth
code "api: a query naming an action" 2 at "$P" "$OB" api GET "$USER_PATH/?cancel_appointment=1" --profile synth
code "api: another host" 2 at "$P" "$OB" api GET https://example.com/x --profile synth
code "api: POST" 2 at "$P" "$OB" api POST "$USER_PATH" --profile synth
code "503 mid-export" 5 at "$Q" "$OB" records export --profile synth --ndjson
value "503 mid-export: trailer" "False" "d['complete']" at "$Q" "$OB" records export --profile synth --ndjson
code "429" 5 at "$Q" "$OB" donations list --profile synth
code "401" 3 at "$R" "$OB" patients list --profile synth
code "302" 3 at "$R" "$OB" readings list --profile synth --test cholesterol
code "server down" 5 at "$DOWN" "$OB" patients list --profile synth
code "a base that is not loopback" 2 env ONEBLOOD_BASE_URL=http://example.com "$OB" patients list --profile synth
code "config path" 0 "$OB" config path
code "config show" 0 "$OB" config show
code "profiles remove without a terminal" 6 "$OB" profiles remove synth
code "profiles remove, unknown name" 4 "$OB" profiles remove nobody --force
code "auth logout" 0 "$OB" auth logout --profile synth
code "a read after logout" 3 at "$P" "$OB" patients list --profile synth
code "auth reset without a terminal" 6 "$OB" auth reset
code "auth reset --force" 0 "$OB" auth reset --force

echo
if [ "$fails" -gt 0 ]; then
    echo "$fails case(s) failed"
    exit 1
fi
echo "every case passed"
