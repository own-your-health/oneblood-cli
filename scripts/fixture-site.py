#!/usr/bin/env python3
"""Serve tests/fixtures/site/ on 127.0.0.1 the way the donor portal answers
oneblood's reads, for driving an installed `oneblood` on synthetic data:

    python3 -I scripts/fixture-site.py 8931 &
    export ONEBLOOD_KEYCHAIN_SERVICE=piekstra.oneblood.dogfood XDG_CONFIG_HOME=$(mktemp -d)
    export ONEBLOOD_BASE_URL=http://127.0.0.1:8931
    oneblood profiles add synth
    printf 'dummy_session=1; userLoggedIn=true' | oneblood auth login --stdin --user-agent test --profile synth
    oneblood patients list --profile synth
    oneblood auth reset --force       # removes the dogfood vault key

A request without the `dummy_session` cookie gets the signed-out answer.
`--status PATH=CODE` answers PATH (path and query, as sent) with that HTTP
status and an empty JSON body, for the error paths; `--set-cookie VALUE`
adds a Set-Cookie to every answer, for the write-back.
"""

import argparse
import http.server
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "..", "tests", "fixtures", "site")
USER = "/api/invita/mobileuser/resource"


def routes():
    r = {USER: "user.json", USER + "/?donations=1": "donations.json"}
    for test in (1, 3, 4, 5, 6, 9):
        r[f"{USER}/?tests={test}"] = f"tests-{test}.json"
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("port", type=int)
    ap.add_argument("--status", action="append", default=[], metavar="PATH=CODE")
    ap.add_argument("--set-cookie")
    args = ap.parse_args()
    statuses = {}
    for s in args.status:
        path, code = s.rsplit("=", 1)
        statuses[path] = int(code)
    table = routes()

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            code, body = 404, b"{}"
            if self.path in statuses:
                code = statuses[self.path]
            elif self.path in table:
                cookie = self.headers.get("Cookie", "")
                name = table[self.path] if "dummy_session=" in cookie else "signed-out.json"
                with open(os.path.join(FIXTURES, name), "rb") as f:
                    body = f.read()
                code = 200
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            if args.set_cookie:
                self.send_header("Set-Cookie", args.set_cookie)
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt, *a):
            sys.stderr.write("fixture-site: " + (fmt % a) + "\n")

    http.server.HTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
