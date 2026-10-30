# Deferred from B7 (OAuth connection)

Issues that cannot be fixed inside the B7 plan. Each says why and what picks it up.

## Owner notifications for OAuth connection events

**Issue.** Owners are not told about `needs_reconnect`, `account_revoked_by_platform`, or the `reconnect_by` warnings at 7 days and 1 day (`reconnect_warned` stages `7d`, `1d`).

**Why not now.** They need `postit-notify`, which D1 creates. B7 records the audit events and the grant and account states, so the information exists; nothing delivers it yet.

**Trigger.** D1: its notification kinds and the `refresh_credentials` cron gain the warning stages.

**Resolved in D1 (Task 14).** `AccountService` now records owner notifications in the same transaction as the state change: `account_needs_reconnect` (`mark_needs_reconnect_in` and the refresh failures), `account_revoked_by_platform` (platform deauthorization events), `reconnect_reminder` at 7 days and 1 day, and `reconnect_by_passed` (the `refresh_credentials` job in `backend/crates/oauth/src/jobs.rs`). Tests: `backend/crates/oauth/tests/notifications.rs`.
