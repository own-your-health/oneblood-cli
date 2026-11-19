# Contributing

1. `make verify` must pass: fmt-check, clippy with warnings denied, the
   tests, the smoke test.
2. Fixtures are synthetic. A new answer shape becomes a file in
   `tests/fixtures/site/` with the real answer's structure and the dummy
   values in `tests/fixtures/README.md`, never a real capture.
3. No personal data anywhere: code, docs, commit messages, pull requests,
   issues. `tests/no_personal_data.rs` enforces the shapes; review the rest.
4. An endpoint change is observed on the signed-in portal first and
   recorded in `docs/api.md`, traps included.
5. Squash merges only, with an approval and green CI.
