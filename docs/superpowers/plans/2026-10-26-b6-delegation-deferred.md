# B6 delegation: deferred issues

## Parties are not notified of grant changes (resolved)

Resolved in plan 03 D1 task 15: `DelegationService` and `expire_delegations` record a `delegation_changed`
notice in the same transaction as each grant, change, revocation, leave, and expiry, and
`AccountService::disconnect` records `limit_emptied` for both parties when a limited grant loses its last
account (`AccountRepo::disconnect` reports them in `Disconnected::emptied_limits`). Tests:
`backend/crates/identity/tests/delegation_notifications.rs`, `backend/crates/oauth/tests/disconnect.rs`.

Notify both parties when a limited grant loses its last account, and the other party on every grant,
change, revocation, leave, and expiry.

Why it cannot be fixed now: in-app notifications need `postit-notify`, which D1 creates. B6 records
each of these events in the audit log only.

Trigger: plan 03 D1. It creates `postit-notify` and wires the delegation service's events to it.

## The generated Dart client clears `expires_at` on a PATCH (open, trigger D6)

The generated Dart client (`mobile/lib/api/`) serializes absent optional fields as `null`, so a
`PATCH /delegations/{id}` built from the generated model clears `expires_at` when the caller meant to
leave it unchanged; the generator has no tri-state option.

Why it cannot be fixed now: B6 has no mobile screens and no caller of this route in `mobile/`.

Trigger: plan 03 D6. The mobile Delegations screen must send the current `expires_at` or build the
PATCH body as a map with only the changed keys.
