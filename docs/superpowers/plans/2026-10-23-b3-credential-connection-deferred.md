# B3 credential connection: deferred items

Issues found while executing the B3 plan that cannot be fixed inside it.

## Import progress for large files

- **Issue:** `POST /accounts/import/{platform}` answers once, after every row is processed (up to `accounts.csv_max_rows` rows, each a platform call), so a large file shows no progress and ties up one request for up to `accounts.csv_timeout`.
- **Why not now:** progress needs a background job with a status route, which is a new feature on the jobs ledger; B3 specifies the single-response route. The service already streams rows lazily (read, verify, connect, skip invalid) with bounded memory, which is the base such a job needs.
- **Trigger:** plan 03 D4 (web Accounts screen). Upload returns an import ID, the rows run as a job, and a `GET` returns processed, connected, and rejected counts plus the report so far.

## Dart client cannot send the CSV import

- **Issue:** `AccountsApi.postApiV1AccountsImportPlatform` in `mobile/lib/api/clients/` is generated with a `String` body and no `Content-Type` header, so Dio sends `application/json` and the server answers 422 (the route requires `text/csv`, as the plan specifies).
- **Why not now:** `swagger_parser` 1.45 offers no per-route exclusion (only `include_paths`) and does not emit a content type for `text/csv`. The generated folder must not be edited by hand, and the app has no import screen.
- **Trigger:** the app gains an account import screen (plan 03 D6, which lists this item). Then call the route through a small non-generated wrapper that sets `Content-Type: text/csv` on the Dio request, or revisit the generator options.
