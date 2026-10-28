# Deferred from B7 (OAuth connection)

Issues that cannot be fixed inside the B7 plan. Each says why and what picks it up.

## Owner notifications for OAuth connection events

**Issue.** Owners are not told about `needs_reconnect`, `account_revoked_by_platform`, or the `reconnect_by` warnings at 7 days and 1 day (`reconnect_warned` stages `7d`, `1d`).

**Why not now.** They need `postit-notify`, which D1 creates. B7 records the audit events and the grant and account states, so the information exists; nothing delivers it yet.

**Trigger.** D1: its notification kinds and the `refresh_credentials` cron gain the warning stages.
