# C1 Contacts and C2 Email Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hold consented contacts with import, export, and suppression, and send a real campaign through a real SMTP server with exact per-recipient results and working unsubscribe.

**Architecture:**
- New `postit-contacts` (plan 01 level 8) is a service over the existing repositories in `postit-data/src/contacts.rs`, plus import, export, and `email_event_apply`.
- New `postit-agent-email` is a direct agent over D1's `DirectSender` flow. It depends on `core`, `http`, and `agent-kit`, never on `data` or `contacts`.
- `postit-api` owns the contacts routes, the public unsubscribe and tracking routes, and the real `LinkSigner`. `postit-server` composes them.

**Tech Stack:** Rust 2024, axum 0.8 (adds the `multipart` feature), sqlx (Postgres), apalis behind `postit-jobs`, askama + Fluent for pages, `csv-async` (workspace), `lettre` 0.11 (workspace), wiremock, utoipa. New workspace dependencies: `mrml` 6 and `hickory-resolver` 0.26, both checked against the dependency rules in their tasks.

**Spec:** `docs/specs/2026-10-30-c1-c2-contacts-email-agent-design.md` (approved). It argues from `docs/plans/03. publishing roadmap.md` (Phases C1 and C2, "Sending an email campaign", the Contacts, Accounts, and Email rows of "REST surface", audit events) and `docs/plans/01. vision and architecture.md` (the `contacts` through `email_events` tables, crate levels 8 and 9, the owner-only list). Read all three first. The spec's decisions are not reopened.

## Global Constraints

- Branch `c1-c2-contacts-email` from `stage`. At plan time, no `*-deferred.md` entry has a C1 or C2 trigger.
- Edition 2024, `rust-version = "1.98"`, workspace lints (`unsafe_code = "forbid"`, clippy `all` + `pedantic`; `unwrap_used`, `expect_used`, `panic` denied outside tests).
- Single migration policy: edit `backend/crates/data/migrations/0001_init.sql` in place; reset the dev database after.
- Crate rules: `contacts` depends on config, core, data, jobs. `postit-agent-email` depends on core, http, agent-kit, plus `lettre`, `mrml`, `hickory-resolver`. No agent depends on `contacts` or `data`. `postit-jobs` stays the only crate that depends on apalis, `postit-http` the only one that builds `reqwest` clients.
- Job payloads carry IDs only. Every job has an ID-only `JobSummary` and an idempotent handler.
- No address, name, token, or message text in a log, job payload, job summary, audit event, import report, notice, or error. Tests assert it through the shared leak harness.
- No database connection is held across an SMTP or DNS call.
- Access levels are the REST surface's Access column: contacts and lists `view`/`edit`; import, export, and removing a suppression owner-only (`403 owner_only` under `X-Postit-Act-As`). Every new route gets the four-caller isolation tests.
- New problem codes, all with web and mobile error-map entries in English and Arabic in the task that adds them: `contact_limit_exceeded` (409), `suppressed` (409), `suppression_not_removable` (409), `status_not_settable` (409), `consent_attestation_required` (400), `csv_invalid` (422), `csv_too_large` (413), `email_address_missing` (409), `live_publish_disabled` (409), `test_send_refused` (409), `capability_unsupported` (409, if it has no entry yet).
- New `ViolationCode`: `email_too_large`, `email_postal_address_required`, `email_merge_tag_invalid`, with client catalog entries.
- Constants: HTML limit `100 * 1024` bytes; import report at most 1,000 entries; stale import age 24 hours; sender-level rule 3 leading `5.7.x` results; at most 2 SMTP session reopens per batch; multipart overhead `64 * 1024` bytes on the import route; test-email handler timeout 120 seconds; token HMAC tag 16 bytes; token labels `postit-unsub-v1`, `postit-click-v1`, `postit-open-v1`.
- Settings: new `[contacts]` with `csv_max_bytes = 52428800` and `csv_max_rows = 200000`; `[jobs.concurrency] contacts = 2`; `email.command_timeout = "30s"`; new `[rate_limit.email_checks]` with `rate_per_minute = 10` and `burst = 5`.
- After every task, from `backend/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and the touched crates' tests. DB tests: `DATABASE_URL` from `!ref/vault/development/postgres.env` with host `127.0.0.1`, `-- --test-threads=3`, `CARGO_TARGET_DIR=K:/cargo-target/postit`. New `sqlx::query!` calls need `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets --all-features` from the crate that gained them. A task that changes the API contract runs `cargo xtask openapi` and commits `backend/openapi.json`, `web/src/api/schema.d.ts`, and `mobile/lib/api/`. Web checks run through PowerShell with pnpm, never npm; `web/CLAUDE.md` has them.
- Commits: Conventional Commits, scope = crate or area, ending with `Co-Authored-By: <model that did the work> <noreply@anthropic.com>`. American English everywhere.

## Review Focus

1. **A CSV from Excel on an Arabic Windows machine:** a UTF-8 BOM, semicolon separators, or Windows-1256 bytes. A BOM and semicolons import correctly; non-UTF-8 bytes answer `422 csv_invalid` rather than storing garbage names. Test in Task 4.
2. **A contact's own data that looks like markup or a merge tag:** `first_name` of `<b>x</b>` or `{{last_name}}`. It renders literally once, and substitution is single-pass, so a value is never expanded again. Test in Task 11.
3. **One address twice in a file with different case, and two imports running at once.** The second row is `duplicate_in_file`, and the concurrent imports neither exceed the cap nor fail on the unique index. Test in Task 4.
4. **The SMTP connection drops halfway through a batch.** Recipients already accepted stay `Sent`, the session reopens up to twice, what is left after that are soft failures, and the batch does not answer as a whole-batch error that would re-send the accepted ones. Test in Task 12.
5. **Unsubscribe after the world moved on:** clicked twice, after the contact was deleted, after `direct_sends` were purged, or after the owner's account was deleted. It answers the success page every time and never a 500. Test in Task 10.
6. **One refused recipient in the middle of a batch.** `lettre`'s `send()` closes the session on any refusal, so the agent never calls it. The next recipient goes out on the same session after `RSET`. Test in Task 12.
7. **A relay refusing the sender, not the recipient.** A 5xx at `MAIL FROM` suppresses nobody, whatever its code. Test in Task 12.
8. **A contact unsubscribes after their address bounced.** The entry becomes `unsubscribed`, so no owner action can remove it. Test in Task 1.

---

## C1

### Task 1: Contact repository fixes

**Files:**
- Modify: `backend/crates/data/src/contacts.rs`
- Modify: `backend/crates/data/src/publishing/direct.rs` (uses of `remove`, if any)
- Test: `backend/crates/data/tests/contacts.rs`

**Interfaces:**
- Produces:
  - `Suppression { id: SuppressionId, reason, at, contact_id: Option<ContactId>, email: Option<String> }` (hash removed from the public shape; add the `SuppressionId` newtype to core).
  - `SuppressionRepo::remove(conn, scope, id: SuppressionId) -> Result<(), DataError>`, `Conflict("suppression_not_removable")` unless `bounced`. It moves a `bounced` contact with that hash to `subscribed`. When that contact is `unsubscribed`, the entry becomes `manual` instead of being deleted, so the owner's unsubscribe survives.
  - `SuppressionRepo::add_hash` keeps its signature with a precedence: `manual` below `bounced` below `unsubscribed` and `complained`, which rank equal. A higher reason replaces a lower one. `Unsubscribed` and `Complained` move `subscribed` and `bounced` contacts; `Bounced` moves `subscribed` contacts only; `Manual` moves none.
  - `NewContact.fields: Option<Value>` and `NewContact.tags: Option<Vec<String>>`. In `upsert`, `None` in `first_name`, `last_name`, `fields`, `tags`, or `locale` keeps what an existing contact has, and a new contact takes the column default.
  - `ContactRepo::update(conn, scope, id, ContactChanges) -> Result<Contact, DataError>`, requiring `Edit`. `ContactChanges { first_name: Option<Option<String>>, last_name: Option<Option<String>>, fields: Option<Value>, tags: Option<Vec<String>>, locale: Option<Option<String>>, consent_source: Option<String>, consent_at: Option<DateTime<Utc>> }`, where the outer `None` leaves a column alone.
  - `ContactRepo::exists(conn, owner, hash: [u8; 32]) -> Result<bool, DataError>`, a system read for the cap.
  - `ContactRepo::set_status` keeps its signature with the rules below.
  - `ContactRepo::lock_owner(tx, owner) -> Result<(), DataError>`, a per-owner `pg_advisory_xact_lock` following `advisory_lock.rs`.

- [ ] **Step 1: Write failing tests** `manual_suppression_imports_as_unsubscribed`, `owner_unsubscribe_survives_delete_and_readd` (`set_status(Unsubscribed)` writes a `manual` suppression), `owner_resubscribe_removes_manual_entry`, `delegate_cannot_resubscribe_manual` (`DataError` maps to `owner_only`), `resubscribe_with_unsubscribed_suppression_conflicts` (`suppressed`), `unsubscribe_of_bounced_contact_refused` (`status_not_settable`), `remove_refuses_non_bounced`, `remove_bounced_resubscribes_contact`, `remove_bounced_keeps_owner_unsubscribe`, `suppression_list_joins_live_contact` (and returns `None` after the contact is deleted), `upsert_none_keeps_existing_values`, `update_changes_only_given_columns`, `exists_matches_by_hash`.
- [ ] **Step 2: Add the Review Focus tests** `unsubscribe_replaces_bounced_entry` (the contact moves to `unsubscribed`, and `remove` answers `suppression_not_removable`), `complaint_replaces_bounced_entry`, `bounce_never_replaces_unsubscribe`.
- [ ] **Step 3: Run them, expect failures.** `cargo test -p postit-data --test contacts`
- [ ] **Step 4: Implement.** In `upsert`, the status `COALESCE` maps `manual` to `unsubscribed`. The update arm uses `COALESCE(EXCLUDED.first_name, contacts.first_name)` and the same for `last_name` and `locale`. `fields` and `tags` are `NOT NULL`, so `VALUES` applies their defaults and the update arm reads the bound parameters: `fields = COALESCE($7, contacts.fields)`. `set_status` to `Unsubscribed` calls `add_hash` with `Manual` in its transaction, changes nothing for a contact already `unsubscribed`, and answers `DataError::Conflict("status_not_settable")` for a `bounced` or `complained` contact. To `Subscribed` it deletes a `manual` entry after `scope.require(Capability::OwnerOnly)`, or conflicts on any other reason; with no entry, `Edit` is enough. `set_status` refuses `Bounced` and `Complained` as targets with the same `status_not_settable`. Update the existing tests and the `add_hash` doc comment for the new precedence.
- [ ] **Step 5: Run them, expect pass.** Refresh the sqlx cache.
- [ ] **Step 6: Commit.** `fix(data): make unsubscribes stick and suppressions removable by ID`

### Task 2: Import staging and stale imports

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql`
- Modify: `backend/crates/data/src/contacts.rs`, `backend/crates/data/src/retention.rs`
- Modify: `backend/crates/config/src/settings.rs`, `backend/crates/config/src/settings_publishing.rs` (where `[limits]` lives), `backend/config/default.toml`
- Modify: `backend/crates/data/tests/schema.rs` (`contact_import_rows` joins `LINKS`)
- Test: `backend/crates/data/tests/contacts.rs`, `backend/crates/data/tests/retention.rs` (or the existing retention test file)

**Interfaces:**
- Produces:
  - Table `contact_import_rows (import_id, owner_id, row_no int, data jsonb, PRIMARY KEY (import_id, row_no))`, composite FK to `contact_imports` with cascade.
  - `contact_imports.list_id UUID NULL`, `FOREIGN KEY (owner_id, list_id) REFERENCES contact_lists (owner_id, id) ON DELETE SET NULL (list_id)`. A list deleted during an import ends the adding to that list and fails nothing.
  - `contact_imports.report` becomes `{"entries": [{"row": n, "code": "…"}], "totals": {"code": n}}`, with that shape as the column default.
  - `ContactImportRepo::create` gains `list: Option<ContactListId>`, and `ContactImport` gains `list_id`.
  - `ContactImportRepo::stage_rows(conn, scope, id, rows: &[StagedRow]) -> Result<(), DataError>` with `StagedRow { row_no: i32, data: Value }`.
  - `ContactImportRepo::set_total(conn, scope, id, rows_total: i32)`.
  - `ContactImportRepo::take_chunk(tx, owner: UserId, id, limit: i64) -> Result<Vec<StagedRow>, DataError>`: selects and deletes `FOR UPDATE SKIP LOCKED`, a system call for the job.
  - `ContactImportRepo::progress(conn, owner, id, ok_delta: i32, rejects: &[ImportRejection]) -> Result<(), DataError>` with `ImportRejection { row_no: i32, code: &'static str }`: adds `rows_ok`, adds `rejects.len()` to `rows_rejected`, adds every reject to `totals`, and appends `entries` up to 1,000. Staging and the job both call it.
  - `ContactImportRepo::complete(conn, owner, id)`: replaces `finish`, keeps the `import_finished` conflict.
  - `ContactImportRepo::abandon(conn, owner, id) -> Result<(), DataError>`: marks a `processing` import `failed` and deletes its staged rows in one statement.
  - `retention::fail_stale_imports(conn, cutoff) -> Result<u64, DataError>`: the same for every `processing` import with `updated_at < cutoff`.
  - `ContactsSettings { csv_max_bytes: u64, csv_max_rows: u32 }` at `settings.contacts`.

- [ ] **Step 1: Write failing tests** `take_chunk_removes_rows_it_returns`, `progress_appends_and_caps_report_at_1000`, `progress_totals_keep_counting_past_1000`, `abandon_fails_import_and_deletes_rows`, `deleting_a_list_clears_list_id`, `stale_import_failed_and_rows_gone`, `running_import_untouched_by_stale_sweep`, `deleting_owner_cascades_staged_rows`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement** the table, the column, the methods, and the settings. Add `fail_stale_imports` to the `data_retention` handler with a 24-hour cutoff, before `purge_contact_imports`.
- [ ] **Step 4: Run them, expect pass.** Refresh the sqlx cache and reset the dev database.
- [ ] **Step 5: Commit.** `feat(data): stage contact imports and fail abandoned ones`

### Task 3: `postit-contacts` service

**Files:**
- Create: `backend/crates/contacts/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/service.rs`
- Modify: `backend/Cargo.toml` (member and workspace dependency)
- Test: `backend/crates/contacts/tests/service.rs`

**Interfaces:**
- Consumes: Task 1, Task 2.
- Produces:
  - `ContactService::new(ContactsDeps { pool, clock, ids, signing_key: Arc<[u8]>, settings: ContactsSettings })`.
  - `create(&self, scope, NewContactInput) -> Result<Contact, ContactError>`; `NewContactInput` requires `consent_source: String` and takes optional `consent_at`.
  - `update(&self, scope, id, ContactPatch) -> Result<Contact, ContactError>`; `ContactPatch` has no email field, and its `status` accepts `Subscribed` and `Unsubscribed` only.
  - `get`, `list`, `delete`; `create_list`, `rename_list`, `delete_list`, `lists`, `add_member`, `remove_member`; `suppressions`, `remove_suppression`.
  - `ContactError` with `Limit`, `InvalidEmail`, `ConsentRequired`, `Suppressed`, `NotRemovable`, `NotFound`, `Forbidden(Capability)`, `Data`.
  - `pub fn parse_address(raw: &str) -> Option<String>` returns the normalized address (use `lettre::Address` or `email_address`, whichever adds less).

- [ ] **Step 1: Write failing tests** `create_requires_consent_source`, `create_refuses_invalid_address`, `cap_blocks_new_contact`, `cap_zero_is_unlimited`, `update_of_existing_ignores_cap`, `concurrent_creates_never_pass_cap` (two tasks at `cap - 1`), `delete_list_audits`, `remove_suppression_audits`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** `create` opens a transaction, calls `ContactRepo::lock_owner`, calls `ContactRepo::exists`, and for a new address reads `count` and `UserLimitsRepo`, then upserts. An address that already exists is not refused by the cap. `update` calls `ContactRepo::update` for the columns and `set_status` for `status`. Audit `contact_list_deleted` and `suppression_removed` with counts and IDs only.
- [ ] **Step 4: Run them, expect pass.**
- [ ] **Step 5: Commit.** `feat(contacts): add the contact service with the contacts cap`

### Task 4: CSV import

**Files:**
- Create: `backend/crates/contacts/src/import.rs`, `src/jobs.rs`
- Modify: `backend/crates/jobs/src/` (the `Queue` enum, `Contacts` beside `Publishing`, and its concurrency setting)
- Test: `backend/crates/contacts/tests/import.rs`

**Interfaces:**
- Consumes: Task 2 repository calls, Task 3 `parse_address`.
- Produces:
  - `ImportOptions { attest_consent: bool, list_id: Option<ContactListId>, columns: ColumnMap }`, `ColumnMap { email: String, consent_source: String, consent_at: Option<String>, first_name: Option<String>, last_name: Option<String>, tags: Option<String>, locale: Option<String>, fields: BTreeMap<String, String> }` (CSV header to field), all `serde`, with `utoipa` behind the crate's `openapi` feature.
  - `ContactService::start_import(&self, scope, options, body: impl AsyncRead + Unpin + Send) -> Result<ContactImport, ContactError>`: stages rows, sets the total, and enqueues in the transaction that ends the upload.
  - `ContactImportJob { import: Uuid, owner: Uuid }`, `JOB_TYPE = "contact_import"`, `Queue::Contacts` (new queue).
  - `ContactService::run_import(&self, job) -> Result<(), JobError>`.
  - `ContactError` gains `AttestationRequired`, `CsvInvalid`, `CsvTooLarge`.

- [ ] **Step 1: Write failing tests** with the spec's reason codes: `maps_columns_and_custom_fields`, `missing_consent_rejected`, `bad_consent_at_rejected`, `row_with_wrong_field_count_rejected`, `mapped_header_missing_answers_csv_invalid`, `attestation_required`, `over_max_rows_refused`, `cap_rejects_new_and_updates_existing`, `suppressed_address_stays_unsubscribed`, `reimport_is_idempotent`, `import_without_name_column_keeps_names`, `empty_cell_keeps_existing_value`, `tags_split_on_pipe`, `leading_quote_before_formula_character_is_stripped`, `import_adds_to_chosen_list`, `list_deleted_during_import_still_completes`, `foreign_list_refused`, `crash_mid_chunk_applies_no_row_twice` (fail after a chunk, rerun, totals exact), `retry_after_completion_changes_nothing`, `staging_rejects_reach_the_report`, `report_holds_no_addresses`.
- [ ] **Step 2: Add the Review Focus tests** `bom_and_semicolons_import`, `windows_1256_bytes_answer_csv_invalid`, `same_address_twice_is_duplicate_in_file` (different case), `two_concurrent_imports_respect_cap`, and `failure_mid_file_leaves_no_staged_rows` (a bad byte at row 5,000 and a file past `csv_max_rows`: the import is `failed`, no row remains, and no job was queued).
- [ ] **Step 3: Run them, expect failures.**
- [ ] **Step 4: Implement.** `start_import` refuses a missing attestation and a foreign list before reading a byte, then reads the header and answers `csv_invalid` when a mapped header is not in the file. Sniff `,` or `;` from the header line; strip a BOM; decode strictly as UTF-8; read with the `flexible` option, so a row with the wrong field count is `row_malformed` rather than a failed file. The reason codes are the spec's plus `consent_at_invalid` and `row_malformed`. An empty cell in an optional column is `None`, an unmapped or empty `consent_at` stays null, and tags split on `|`. A leading `'` before `=`, `+`, `-`, `@`, tab, or CR is removed, which undoes Task 5's neutralizing. Duplicates are detected at staging against a `HashSet` of address hashes for the file. Stage 1,000 rows at a time, calling `progress` with that chunk's rejects. Any error after the import row exists calls `abandon` before it returns, and a client that disconnects is left to the stale sweep. The job loop: open a transaction, `lock_owner`, `take_chunk(1000)`, apply, `progress`, commit, until empty, then `complete` and audit `contacts_imported` with counts and the attestation in one transaction. A list gone by then adds no members. An `Err` from a chunk rolls it back, so the rows stay.
- [ ] **Step 5: Run them, expect pass.**
- [ ] **Step 6: Commit.** `feat(contacts): import contacts from CSV through a staged job`

### Task 5: Export, `email_event_apply`, and job wiring

**Files:**
- Create: `backend/crates/contacts/src/export.rs`
- Modify: `backend/crates/contacts/src/jobs.rs`, `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/contacts.rs`, `backend/crates/data/src/publishing/direct.rs`, `backend/crates/server/src/compose.rs`, `backend/crates/server/Cargo.toml`, `backend/crates/server/src/lib.rs`
- Test: `backend/crates/contacts/tests/export.rs`, `backend/crates/contacts/tests/events.rs`

**Interfaces:**
- Produces:
  - `ContactService::export(&self, scope, list: Option<ContactListId>) -> Result<impl Stream<Item = Result<Bytes, ContactError>>, ContactError>`, owner-only. It counts the rows and audits `contacts_exported` before the first byte, so a dropped download is still audited.
  - Export columns: `email`, `first_name`, `last_name`, `tags` (joined with `|`), `locale`, `consent_source`, `consent_at`, `status`, then one column per custom field key in sorted order.
  - `ContactRepo::field_keys(conn, owner, list: Option<ContactListId>) -> Result<Vec<String>, DataError>`: the distinct `fields` keys, for the header row.
  - `ContactRepo::page_after(conn, owner, list: Option<ContactListId>, after: Option<ContactId>, limit: i64) -> Result<Vec<Contact>, DataError>`: keyset paging by ID.
  - `EmailEventApply { owner: Uuid, send: Uuid, kind: EmailEventKind }`, `JOB_TYPE = "email_event_apply"`, `Queue::Contacts`.
  - `CREATE UNIQUE INDEX email_events_once_idx ON email_events (direct_send_id, kind) WHERE kind NOT IN ('clicked', 'opened')`.
  - `DirectRepo::record_event(conn, owner, send, kind, link_index: Option<i32>, at) -> Result<bool, DataError>`: `ON CONFLICT DO NOTHING`, so a repeat of a once-only kind changes nothing and answers `true`; `false` when the send row is gone.
  - `DirectRepo::send_hash(conn, owner, send) -> Result<Option<[u8; 32]>, DataError>`.
  - `EmailEventKind` in core: `Delivered`, `Bounced`, `Complained`, `Clicked`, `Opened`, `Unsubscribed`.

- [ ] **Step 1: Write failing tests** `export_round_trips_through_import` (custom fields and tags included), `export_round_trips_formula_cells`, `export_by_list`, `export_neutralizes_formula_cells` (a cell starting `=`, `+`, `-`, `@`, tab, or CR gets a leading `'`), `export_audits_before_the_first_byte`, `delegate_export_refused`, `bounce_event_suppresses`, `complaint_event_suppresses_as_complained`, `event_for_purged_send_is_harmless`, `apply_twice_records_one_event`, `repeat_clicks_keep_every_row`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** Export reads `field_keys`, writes the header, then calls `page_after`, holding no connection between pages. Register both jobs, their `JobSummary`s, and the `contacts` concurrency in `compose::registry`.
- [ ] **Step 4: Run them, expect pass.**
- [ ] **Step 5: Commit.** `feat(contacts): export contacts and apply email events`

### Task 6: Contacts routes

**Files:**
- Create: `backend/crates/api/src/routes/contacts.rs`, `backend/crates/api/src/dto_contacts.rs`
- Modify: `backend/crates/api/src/routes/mod.rs`, `src/settings.rs`, `src/state.rs`, `src/openapi.rs`, `src/error.rs`, `backend/Cargo.toml` (axum `multipart`), `backend/crates/server/src/compose.rs`
- Modify: `web/src/api/errors*` and the mobile error map, with generated clients
- Test: `backend/crates/api/tests/contacts.rs`, `backend/crates/api/tests/contacts_leak.rs`

**Interfaces:**
- Consumes: Tasks 3 to 5.
- Produces: every Contacts and suppression route of the REST surface, in `routes::contacts`. `POST /contacts/import` takes multipart parts `options` (JSON `ImportOptions`) and `file`, in that order, with `Idempotency-Key` accepted, and answers `202` with the import. `GET /contacts/export` answers `text/csv` as a stream. Both have per-user rate limits from the existing settings pattern.
- `ApiSettings.contacts_csv_max_bytes: usize` from `contacts.csv_max_bytes`.

- [ ] **Step 1: Write failing tests:** the four-caller matrix over every route against the Access column; `import_without_attestation_400`; `import_over_2_mib_succeeds`; `import_over_max_bytes_413` (`csv_too_large`, from the handler's reader); `import_declared_far_over_limit_413` (`payload_too_large` with `max_bytes`, from the layer); `import_file_before_options_422`; `import_replay_returns_the_same_import`; `import_body_outlives_request_timeout` and `export_outlives_request_timeout` (a slow body and a slow reader past `server.request_timeout`); `import_progress_visible`; `patch_cannot_change_email` (unknown field refused); `patch_bounced_status_refused`; `suppression_delete_non_bounced_409`; `error_codes_are_stable` for each new code; `contacts_leak`, where no address appears in logs at any level.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement** thin handlers in the `delegations.rs` style, except the two streaming routes, which go in `v1_custom_limits_router` and not in the default group, whose body limit is 1 MiB and whose timeout is 30 seconds. `POST /contacts/import` uses `with_streaming_limits(router, contacts_csv_max_bytes + 64 KiB, media_idle_timeout)` plus `DefaultBodyLimit::disable()`, because axum's `Multipart` extractor keeps its own 2 MB cap and `with_streaming_limits` does not lift it. A counting reader in the service answers `csv_too_large` at `csv_max_bytes`, so the layer only refuses a declared `Content-Length` far beyond it. `GET /contacts/export` uses `with_streaming_limits(router, 0, media_idle_timeout)`. The import handler reads the `options` part first (422 `validation_failed` when another part comes first), then calls `idempotency::run` with `request_hash` over the `options` JSON and `Content-Length`, as `media::upload` does, because the streamed file cannot be hashed. Map `ContactError` in `error.rs`.
- [ ] **Step 4: Run them, expect pass.**
- [ ] **Step 5: Regenerate clients** with `cargo xtask openapi`, then add the new codes to the web and mobile maps in English and Arabic. Run `cargo xtask openapi --check`.
- [ ] **Step 6: Commit.** `feat(api): add the contacts, lists, import, export, and suppression routes`

---

## C2

### Task 7: Core contract changes

**Files:**
- Modify: `backend/crates/core/src/email.rs`, `src/agent/capability.rs`, `src/agent/context.rs`, `src/agent/client.rs`, `src/validation/violation.rs`, `src/validation/engine.rs`
- Modify: `backend/crates/agents/mock/src/capabilities/direct.rs`, `backend/crates/agent-kit/src/testkit/` (fakes, context, conformance), `backend/crates/agent-kit/tests/support/echo.rs`
- Modify: `backend/crates/publishing/src/direct.rs`, the `assemble` module, `backend/crates/oauth/src/service.rs`, `src/testkit.rs`, `backend/crates/server/src/compose.rs`, `src/unavailable.rs`
- Test: existing suites, plus `backend/crates/publishing/tests/direct.rs`

**Interfaces:**
- Produces:
  - `EmailLanguage { EnUs ("en-US"), Ar ("ar") }`, and `EmailDocument.language: EmailLanguage`, required.
  - `pub const TEST_SEND_ID: DirectSendId` (the nil UUID).
  - `DirectSender::send_batch(&self, account, recipients, variant, assets: &[MediaAsset])`.
  - `LinkSigner::unsubscribe_url(&self, recipient, language: EmailLanguage) -> Url`.
  - `trait LinkSignerSource: Send + Sync { fn for_owner(&self, owner: UserId) -> Arc<dyn LinkSigner>; }`; `AccountServiceDeps.link_signer: Arc<dyn LinkSignerSource>`.
  - The doc comments on `DirectSendOutcome`'s `reason` fields say a short code, never server text, because `fate()` stores it in `direct_sends.error_code`.
  - `Capability::SenderDomainCheck`, `trait SenderDomainCheck { async fn check(&self, account: &AccountRef, dkim_selector: Option<&str>) -> Result<Vec<DomainFinding>, AgentError>; }`, `DomainFinding { record: DomainRecord (Spf, Dkim, Dmarc), status: DomainStatus (Ok, Missing, Weak, Unknown), code: String }`, and `AgentClient::domain_check(&self) -> Option<&dyn SenderDomainCheck>` defaulting to `None`.
  - The Global Constraints' new `ViolationCode`s.
  - `take_audience` answers `audience_invalid` for `options.lists` when no list is named.

- [ ] **Step 1: Write failing tests** `email_document_requires_language` (wire format), `run_batch_passes_email_block_assets` (the mock records the asset IDs it got), `assemble_loads_email_block_assets`, `empty_audience_refused`, `signer_is_bound_per_owner` (fake source records the owner).
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** `run_batch` keeps `assembled.assets` beside the variant. Update every `EmailDocument` literal and D1's no-list direct tests. `UnavailableLinks` implements `LinkSignerSource` until Task 9 deletes it.
- [ ] **Step 4: Run** `cargo test -p postit-core -p postit-agent-kit -p postit-agent-mock -p postit-publishing -p postit-oauth`, expect pass.
- [ ] **Step 5: Regenerate clients, add the violation entries to the web and mobile catalogs.**
- [ ] **Step 6: Commit.** `feat(core): extend the direct contract for the email agent`

### Task 8: Media kept alive for sent mail

**Files:**
- Modify: `backend/crates/data/migrations/0001_init.sql`, `backend/crates/data/src/media.rs`, `backend/crates/data/src/publishing/direct.rs`, `backend/crates/publishing/src/direct.rs`, `backend/crates/publishing/src/worker.rs`, `backend/crates/publishing/src/lib.rs` (deps), `backend/crates/server/src/compose.rs`
- Test: `backend/crates/data/tests/media.rs`, `backend/crates/publishing/tests/direct.rs`

**Interfaces:**
- Produces:
  - `media_assets.retain_until TIMESTAMPTZ NULL`.
  - `MediaRepo::retain(tx, owner, assets: &[MediaAssetId], until) -> Result<(), DataError>`: sets `GREATEST(retain_until, until)`.
  - `PublishingDeps.email_image_ttl: Duration` from `email.image_url_ttl`.
  - `direct::start` gains `images: &[MediaAssetId]`, passed by `worker.rs` from the assembled variant.

- [ ] **Step 1: Write failing tests** `email_block_asset_refused_while_job_active`, `retained_asset_refused_until_ttl`, `retention_survives_post_deletion`, `retain_never_moves_earlier`, `start_sets_retain_until_for_document_images`, `dry_run_retains_nothing`, `each_batch_extends_retain_until` (a campaign that runs across a cap crossing: the TTL counts from the last send), `user_deletion_ignores_retain_until`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** Extend the `jsonb_path_exists` query with `$.variants[*].email.blocks[*].asset`; refuse `media_in_use` when `retain_until > now()`; call `retain` from `start` in the materialize transaction, with the IDs from `variant.email.media_ids()`, and again in `Leased::record_after`'s transaction when the batch recorded a `Sent` row, because each batch signs its URLs for the full TTL.
- [ ] **Step 4: Run them, expect pass.** Refresh the sqlx cache.
- [ ] **Step 5: Commit.** `fix(media): keep email images alive for the signed URL lifetime`

### Task 9: Tokens and the real `LinkSigner`

**Files:**
- Create: `backend/crates/api/src/email_links.rs`
- Delete: `backend/crates/server/src/unavailable.rs`
- Modify: `backend/crates/api/src/lib.rs`, `backend/crates/server/src/compose.rs`, `src/lib.rs`
- Test: unit tests in `email_links.rs`

**Interfaces:**
- Produces:
  - `EmailLinks::new(key: &[u8], base: Url)`, `impl LinkSignerSource`.
  - `Unsubscribe { owner: UserId, hash: [u8; 32], language: EmailLanguage, send: DirectSendId }`, `Click { send: DirectSendId, destination: Url }`, `Open { send: DirectSendId }`.
  - `EmailLinks::{verify_unsubscribe, verify_click, verify_open}(&self, token: &str) -> Option<…>`.
  - URLs under `{email.tracking_base_url or server.public_url}/public/u/{token}`, `/public/t/c/{token}`, `/public/t/o/{token}`.

- [ ] **Step 1: Write failing tests** `each_kind_round_trips`, `tampered_token_refused`, `unsub_token_refused_as_click` and the other two crossings, `token_is_not_the_address_hash`, `token_never_expires` (no clock in the payload), `tag_compared_in_constant_time` (uses `Mac::verify_truncated_left`).
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** The tag is `HMAC-SHA256(key, label || 0x00 || payload)` truncated to 16 bytes. The token is URL-safe unpadded base64 of `payload || tag`. The hash comes from `postit_data::email_hash` over `recipient.address`.
- [ ] **Step 4: Run them, expect pass.**
- [ ] **Step 5: Commit.** `feat(api): sign unsubscribe and tracking links`

### Task 10: Public email routes

**Files:**
- Create: `backend/crates/api/src/routes/public_email.rs`, `backend/crates/api/templates/unsubscribe.html`, `backend/crates/api/templates/unsubscribed.html`, `backend/crates/api/templates/unsubscribe_test.html`, `backend/crates/api/templates/link_invalid.html`
- Modify: `backend/crates/api/src/router.rs`, `src/middleware.rs`, `src/routes/mod.rs`, `backend/crates/i18n/` catalogs, the request-tracing redaction list
- Test: `backend/crates/api/tests/public_email.rs`, `backend/crates/api/tests/public_email_leak.rs`

**Interfaces:**
- Consumes: Task 9; Task 5's `DirectRepo::record_event`; `SuppressionRepo::add_hash` with `OwnerScope::own(owner)`.
- Produces: `public_email_router(&settings)`, merged beside `public_media_router`. The `signed_public` check in `middleware.rs` also covers `/public/u/` and `/public/t/`. Each handler marks `Authenticated` once the token verifies, so a good token never penalizes its IP, and a bad one does.
- HTML pages carry `Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'`, `Referrer-Policy: no-referrer`, and `Cache-Control: no-store`.

- [ ] **Step 1: Write failing tests** `get_changes_nothing` (shows the confirm page), `post_suppresses_and_marks_contact`, `rfc8058_one_click_post` (form body `List-Unsubscribe=One-Click`), `one_click_ignores_the_body` (any or no body), `tampered_token_404_page`, `page_in_token_language` (Arabic has `dir="rtl"`), `pages_carry_security_headers`, `test_token_suppresses_nothing` (`TEST_SEND_ID`), `click_redirects_to_signed_url_only`, `click_records_event`, `click_after_purge_still_redirects`, `open_pixel_returns_gif_and_records`, `bad_tokens_penalize_the_ip`, `good_tokens_are_never_limited` (mail proxies and offices share one address), `over_limit_click_redirects_without_recording`, and the leak test, where no token appears in logs or error bodies.
- [ ] **Step 2: Add the Review Focus test** `unsubscribe_survives_everything`: POST twice, after the contact is deleted, after `direct_sends` are purged, and after the owner is deleted. Each answers the success page. A foreign-key failure for a deleted owner is treated as done.
- [ ] **Step 3: Run them, expect failures.**
- [ ] **Step 4: Implement** with askama and Fluent, English and Arabic. The redirect is `303`. The pixel is a fixed 1x1 GIF with `Cache-Control: no-store`. The unsubscribe `POST` reads no body. A valid click or open always answers; it records its event only when `state.limits.ip.check(&ip)` allows, so the per-IP limit bounds event rows without breaking a link.
- [ ] **Step 5: Run them, expect pass.**
- [ ] **Step 6: Commit.** `feat(api): add the public unsubscribe and tracking routes`

### Task 11: Email rendering

**Files:**
- Create: `backend/crates/agents/email/Cargo.toml`, `src/lib.rs`, `src/render/mod.rs`, `src/render/merge.rs`, `src/render/mjml.rs`, `src/render/text.rs`, `src/render/footer.rs`
- Modify: `backend/Cargo.toml`
- Test: unit tests and `backend/crates/agents/email/tests/render.rs` with `insta` snapshots, if the workspace uses it; otherwise the snapshot files the `mail` crate uses

**Interfaces:**
- Produces:
  - `MergeTemplate::parse(&str) -> Result<MergeTemplate, MergeError>` and `MergeTemplate::render(&self, fields: &BTreeMap<String, String>, escape: Escape) -> String`, with `Escape::{Html, Header, None}`.
  - `Skeleton::compile(doc: &EmailDocument, images: &HashMap<MediaAssetId, Url>, sender: &Sender) -> Result<Skeleton, RenderError>`.
  - `Skeleton::for_recipient(&self, recipient: &DirectRecipient, links: &dyn LinkSigner, open_tracking: bool) -> RenderedMail`.
  - `Skeleton::worst_case_html_bytes(&self) -> usize`: the compiled length with every link slot at the signed length of its token, which is what `email_too_large` measures. Merge values count as empty.
  - `RenderedMail { subject: String, html: String, text: String, unsubscribe: Url }`.
  - `Sender { from_name, from_address, reply_to: Option<String>, postal_address }`.

- [ ] **Step 1: Check the dependency.** Confirm `mrml` 6's license is permissive, that its async features can be off, and that its name passes the dependency rule. Stop and raise it if not.
- [ ] **Step 2: Write failing tests** `merge_with_fallback`, `missing_field_without_fallback_is_empty`, `subject_strips_crlf`, `merge_tag_in_url_rejected` (raw `{{` in a query and `%7B%7B` in a path, because `Url` encodes braces only in the path), `document_text_is_escaped` (`<`, `&`, and `<mj-include path="…">` in a text block render as text), `worst_case_bytes_count_signed_links`, `footer_always_present`, `footer_arabic_rtl_for_ar_document`, `recipient_locale_does_not_change_direction`, `plain_text_built_from_blocks`, `buttons_rewritten_through_click_url`, `mailto_not_rewritten`, `no_pixel_unless_enabled`, `test_send_skips_rewriting`, and snapshots at desktop and mobile widths in both directions.
- [ ] **Step 3: Add the Review Focus test** `contact_data_is_inert`: a `first_name` of `<b>x</b>` renders escaped, and one of `{{last_name}}` renders literally.
- [ ] **Step 4: Run them, expect failures.**
- [ ] **Step 5: Implement.** Document text is XML-escaped before it enters the MJML source, so no block can add a tag or an `mj-include`. MJML compiles once with sentinels from a random per-compile prefix, so document text cannot collide with them. `for_recipient` does one left-to-right pass over the compiled string and writes values straight to the output, never scanning them again.
- [ ] **Step 6: Run them, expect pass.**
- [ ] **Step 7: Commit.** `feat(agent-email): render email documents to HTML and text`

### Task 12: SMTP transport and `send_batch`

**Files:**
- Create: `backend/crates/agents/email/src/transport.rs`, `src/smtp.rs`, `src/classify.rs`, `src/client.rs`, `src/credentials.rs`, `src/descriptor.rs`
- Modify: `backend/crates/agents/email/src/lib.rs`, `backend/crates/config/src/settings.rs` (`email.command_timeout`, and the doc comment on `allow_private_hosts`), `backend/crates/config/src/loader.rs` (its inline defaults), `backend/config/default.toml`, `backend/config/development.toml`, `backend/crates/server/src/compose.rs`, the `lettre` features in `backend/Cargo.toml`
- Test: `backend/crates/agents/email/tests/send.rs`, `tests/classify.rs`, with an in-process SMTP sink in `tests/support/`

**Interfaces:**
- Consumes: Task 7's contract, Task 11's renderer.
- Produces:
  - `trait EmailTransport: Send + Sync { async fn open(&self) -> Result<Box<dyn EmailSession>, TransportError>; }`, `trait EmailSession: Send { async fn send(&mut self, mail: &OutgoingMail) -> Result<SentMail, SendFailure>; }`.
  - `SendFailure::{Refused(SendRefusal), Connection(TransportError)}`. `SendRefusal { stage: SmtpStage, code: u16, enhanced: Option<(u8, u16, u16)> }`, with `SmtpStage::{Mail, Rcpt, Data}`. It holds no server text, because replies often echo the address.
  - `classify(&SendRefusal) -> Classified`, with `Classified::{Bounce, Soft, Rejected, Sender}`. The 3-leading rule is batch state in `send_batch`, not a classification.
  - `reason` in every `DirectSendOutcome` is a fixed code: `smtp_5_1_1` from an enhanced code, otherwise `smtp_550`.
  - `EmailAgent::new(EmailAgentConfig { allow_private: bool, image_url_ttl: Duration, command_timeout: Duration })`, with platform ID `email`. The client builds Task 11's image map through `MediaHost::public_url` with that TTL, from the `assets` argument.
  - A `Resolve` seam inside the transport, defaulting to `postit_http::resolve_checked`, which tests replace with `postit_http::testkit::StubLookup`.
  - The descriptor's options schema declares `open_tracking` (boolean, default false).
  - Credentials: `transport` (`smtp`), `from_name`, `from_address`, `reply_to`, `postal_address`, `host`, `port`, `tls` (`none`, `starttls`, `tls`), `username`, `password`. `tls = none` is refused at connect unless `allow_private` is on, so a password never crosses a public network in clear.

- [ ] **Step 1: Use this `lettre` route, checked against the 0.11.23 source.** Connect with `AsyncSmtpConnection::connect_tokio1(socket_addr, Some(command_timeout), &hello, tls, None)`. A `SocketAddr` needs no lookup, and `TlsParameters::new(host)` sets the TLS name from the original host. Pass `Some` for `tls`, or `None` and then call `starttls` for `starttls`, where a server without STARTTLS is an error and never a plain fallback. Do not use `connect_with_transport`: it sends EHLO before implicit TLS can start, and `AsyncNetworkStream` is deprecated, which fails `-D warnings`. Never pass `lettre` a host name to resolve.
- [ ] **Step 2: Write failing classification tests** using the spec's table, per stage. At `Rcpt`: `550 5.1.1`, plain `551`, `553`, and `550`, and `5.2.1` are `Bounce`; `550 5.7.1`, `554`, and `5.1.7`, `5.1.8` (sender codes) are `Rejected`; `450`, `421` are `Soft`. At `Mail`: any 5xx is `Sender`, and 4xx is `Soft`. At `Data`: any 5xx is `Rejected`, and 4xx is `Soft`. Enhanced codes are parsed from the error's response text; `lettre` exposes only the basic code.
- [ ] **Step 3: Write failing send tests** against the sink: `multipart_html_and_text`, `unsubscribe_headers_present`, `reply_to_set`, `message_id_from_send_id`, `test_send_has_no_unsubscribe_headers`, `test_sends_get_distinct_message_ids`, `permanent_rcpt_is_bounced`, `temporary_rcpt_is_soft`, `three_leading_5_7_is_authentication_error`, `5_7_after_a_success_stays_per_recipient`, `auth_failure_is_authentication_error`, `connection_refused_is_retryable`, `stalled_server_times_out`, `reason_codes_hold_no_server_text` (the sink echoes the address in its reply), `private_host_refused`, `private_host_allowed_with_flag`, `connects_to_checked_address` (the transport never performs a second lookup; inject the lookup and count), `second_address_used_when_the_first_refuses`, `starttls_missing_is_an_error`, `tls_none_refused_without_allow_private`, `validate_requires_postal_address`, `validate_refuses_oversized_html`, `verify_credentials_authenticates`.
- [ ] **Step 4: Add the Review Focus tests.** The sink is strict: it answers `503` to `MAIL` while a transaction is open, and it can refuse at any stage.
  - `refused_recipient_keeps_the_session`: a `550 5.1.1` on the second of three recipients. The third is sent on the same connection after `RSET`.
  - `mail_refusal_suppresses_nobody`: `553 5.1.8` at `MAIL` with nothing accepted answers `AgentError::Authentication`.
  - `mail_refusal_after_a_success_is_soft`: accepted recipients stay `Sent`, and that recipient and the rest are `Failed { retryable: true }`.
  - `connection_drop_mid_batch`: the sink closes after the second recipient, once. Accepted recipients stay `Sent`, the rest go out on a reopened session, and `send_batch` answers `Ok`.
  - `connection_keeps_dropping`: after two reopens, the recipients left are `Failed { retryable: true }`, and `send_batch` answers `Ok`.
- [ ] **Step 5: Run them, expect failures.**
- [ ] **Step 6: Implement.** Resolve through `postit_http::resolve_checked` with `AddressPolicy { allow_private }`, and try each checked address in order. `EmailSession::send` never calls `AsyncSmtpConnection::send`, which aborts the session on any refusal. It calls `command(Mail)`, `command(Rcpt)`, `command(Data)`, and `message()` itself, copies `send()`'s SMTPUTF8 and 8BITMIME parameters, tags a refusal with its stage, and calls `command(Rset)` after one. Every command and the connect run under `tokio::time::timeout(command_timeout)`, because `lettre`'s async timeout covers the connect only and a stalled relay would otherwise hold the batch's lease forever; expiry is `SendFailure::Connection`. Send sequentially, with one session per batch. On `SendFailure::Connection` or a `421` reply, that recipient gets `Failed { retryable: true }` and the session reopens, at most twice per batch; after that, every recipient left gets the same result. A `Sender` result with nothing accepted answers `AgentError::Authentication`, and after a success it ends the batch with the rest `Failed { retryable: true }`. A test send uses a random `Message-ID`, because `TEST_SEND_ID` is fixed and Gmail drops or threads repeats. Pass `email.command_timeout` from `compose`. Set `allow_private_hosts = true` in `development.toml`.
- [ ] **Step 7: Run them, expect pass.**
- [ ] **Step 8: Commit.** `feat(agent-email): send campaigns over SMTP with bounce classification`

### Task 13: Server wiring and the end-to-end proof

**Files:**
- Modify: `backend/crates/server/Cargo.toml` (`default = ["email"]`, `email = ["dep:postit-agent-email"]`), `backend/crates/server/src/compose.rs` (`agent_registry(&Settings)`, and its `unused_mut` allowance becomes `not(any(feature = "mock-agent", feature = "email"))`), `src/lib.rs`, `backend/crates/server/tests/agents.rs`, `backend/crates/server/tests/deletion.rs`
- Create: `backend/crates/agents/email/tests/conformance.rs`, `backend/crates/server/tests/email_campaign.rs`

**Interfaces:**
- Consumes: Tasks 3, 7 to 12.
- Produces: `compose::agent_registry(settings: &Settings) -> anyhow::Result<AgentRegistry>`.

- [ ] **Step 1: Write failing tests:** conformance in direct mode; in `email_campaign.rs`, through `AccountService` and `PublishingService` against the sink: `campaign_reaches_every_subscribed_contact`, `suppressed_contact_is_skipped`, `unsubscribe_link_from_the_sent_mail_works` (read the URL out of the captured message and POST it), `per_recipient_results_are_exact`, `cap_crossing_continues_next_day`, `sender_refusal_pauses_and_resumes_after_reconnect`, `no_address_in_logs`, `no_address_in_error_codes` (every `direct_sends.error_code` after a run with a sink that echoes addresses), and, in `tests/agents.rs`, `registry_for_production_settings_holds_email`. That test calls `agent_registry` only: the self dev-dependency compiles the mock into every server test, so `refuse_mock_in_production` would refuse a real production start here.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement the wiring.**
- [ ] **Step 4: Run them, expect pass.** Also `cargo build -p postit-server`, which is the production feature set without the mock, and `cargo build -p postit-server --no-default-features`, so the build without agents still compiles.
- [ ] **Step 5: Commit.** `feat(server): register the email agent`

### Task 14: Test email route

**Files:**
- Modify: `backend/crates/api/src/routes/accounts.rs`, `src/routes/mod.rs`, `src/limit.rs`, `src/dto_accounts.rs`, `src/openapi.rs`, `backend/crates/config/src/settings.rs`, `backend/config/default.toml`, `backend/crates/oauth/src/service.rs` (if `bound_client` is not reachable), and the media lookup for assets
- Test: `backend/crates/api/tests/accounts.rs`

**Interfaces:**
- Produces: `POST /accounts/{id}/test-email`, body `{ "email": EmailDocument }`, `204` on success. Owner-only, limited by `rate_limit.email_checks`. It sits in `v1_custom_limits_router` under `with_limits(router, body_limit, 120 seconds)`, because `connect` and each SMTP command may take `email.command_timeout`.

- [ ] **Step 1: Write failing tests** `sends_to_the_acting_users_address`, `no_row_and_no_cap_used`, `delegate_refused`, `non_direct_account_409`, `user_without_address_409` (`email_address_missing`, since `users.email` is nullable), `kill_switch_409` (`live_publish_disabled`), `disabled_platform_409`, `invalid_document_422`, `agent_error_code_surfaces`, `refused_recipient_409` (`test_send_refused`, with the reason code in `detail`), `unknown_media_asset_404`, `rate_limited`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** Read `AppSettingsRepo::live_publishing` first, treating an unseeded row as off, as the worker does. Use `TEST_SEND_ID`, the signed-in user's own email, and a `Variant` built from the body, with assets loaded from `MediaRepo`. Run `validate_target` and the client's `validate` before sending. `send_batch` answers `Ok` for a per-recipient refusal, so map a `Bounced` or `Failed` result to `test_send_refused`.
- [ ] **Step 4: Run them, expect pass.** Regenerate clients.
- [ ] **Step 5: Commit.** `feat(api): send a test email to the acting user`

### Task 15: DNS check

**Files:**
- Create: `backend/crates/agents/email/src/dns.rs`
- Modify: `backend/crates/agents/email/src/client.rs`, `src/descriptor.rs`, `backend/crates/api/src/routes/accounts.rs`, `src/dto_accounts.rs`, `src/openapi.rs`, `backend/Cargo.toml`
- Test: `backend/crates/agents/email/tests/dns.rs`, `backend/crates/api/tests/accounts.rs`

**Interfaces:**
- Consumes: Task 7's `SenderDomainCheck`.
- Produces: `trait TxtLookup: Send + Sync { async fn txt(&self, name: &str) -> Result<Vec<String>, LookupError>; }` with a `hickory-resolver` implementation; `GET /accounts/{id}/dns-check?dkim_selector=…`, owner-only, limited by `rate_limit.email_checks`. A `dkim_selector` that is not a valid DNS label answers 422 `validation_failed`.

- [ ] **Step 1: Write failing tests** with a fake `TxtLookup`: `spf_found`, `spf_missing`, `spf_plus_all_is_weak`, `dmarc_p_none_is_weak`, `dmarc_missing`, `dkim_unknown_without_selector`, `dkim_ok_with_selector`, `lookup_failure_is_unknown_not_error`; on the API, `mock_account_409_capability_unsupported`, `delegate_refused`, `bad_selector_422`, `rate_limited`.
- [ ] **Step 2: Run them, expect failures.**
- [ ] **Step 3: Implement.** Check `hickory-resolver`'s license and that it builds without OpenSSL. Stop and raise it if not. The domain comes from the sender's address. Finding codes: `spf_ok`, `spf_missing`, `spf_weak`, `dkim_ok`, `dkim_missing`, `dkim_unknown`, `dmarc_ok`, `dmarc_missing`, `dmarc_weak`, plus web and mobile catalog entries.
- [ ] **Step 4: Run them, expect pass.** Regenerate clients.
- [ ] **Step 5: Commit.** `feat(agent-email): check the sending domain's DNS records`

### Task 16: Documents and closing gates

**Files:**
- Modify: `docs/plans/03. publishing roadmap.md`, `docs/plans/01. vision and architecture.md` (only where a stated fact changed), `docs/configuration.md`, `CLAUDE.md`
- Create: `docs/plans/2026-10-30-c1-c2-contacts-email-agent-verification.md`, plus `-deferred.md` only if something truly cannot be fixed here

- [ ] **Step 1: Update the roadmap:** a `GET` on the unsubscribe link does not unsubscribe, the staged 5xx classification, the sender-level pause, the suppression precedence, `EmailDocument.language`, `retain_until` counted from the last send, the one-event rule for `email_events`, the public route limits, and the suppressions route response.
- [ ] **Step 2: Update `docs/configuration.md`** (`[contacts]`, `email.command_timeout`, `[rate_limit.email_checks]`, the wider `email.allow_private_hosts`) and `CLAUDE.md` (C1 and C2 built, the crate list, and that a build now has the email agent by default).
- [ ] **Step 3: Write the verification file:** a campaign through real Papercut, viewed in its UI; rendering in Gmail, Outlook, and Apple Mail, including Arabic; a one-click unsubscribe from a real mail client. Each with why an agent cannot do it and its trigger (D3 for the Papercut run, G1 for real clients).
- [ ] **Step 4: Run the full gates** from `backend/`: `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo check --workspace --all-targets`; `cargo test --workspace`; `cargo deny check`; `cargo xtask openapi --check`. Then the web and mobile gates. Expected: all pass.
- [ ] **Step 5: Commit.** `docs: record C1 and C2 and what they changed`
