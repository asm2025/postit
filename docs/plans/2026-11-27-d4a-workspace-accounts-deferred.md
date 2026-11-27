# D4a deferred items

Issues found while executing the D4a plan that cannot be fixed inside it.

## The `identity_unreachable` startup reason is never raised

- **Issue:** `StartupFailure` and the startup screen know the reason `identity_unreachable` ("The sign-in service cannot be reached.") and the catalog has its sentence, but no startup step raises it. The runtime config and the auth config are the only things startup loads, and both have their own reasons.
- **Why not now:** raising it means a request to the identity provider at every startup. That would stop a user with a valid session from opening the app while the identity provider is briefly down, though the API would answer them. Whether sign-in-time or startup-time is the right place to detect it is a product call, not a D4a one.
- **Trigger:** a plan that touches sign-in or session renewal (D6's shared auth work, or G5 hardening). Decide there where a failed provider lookup is reported, then raise this reason at that place.

## The checklist countdown tick has no test

- **Issue:** `DiscoveryChecklist` re-reads the minutes left every 15 seconds. The tests render it with an expiry in the past and one ten minutes out, but none advances a clock, so a tick that stopped firing would not fail a test.
- **Why not now:** MSW's request handling and fake timers interfere in this test setup; a clean test needs a harness change (a clock the component receives, as the backend's `TestClock` does) that no D4a screen needs.
- **Trigger:** the next plan that adds a timer-driven screen (D4b's composer autosave, or D5's calendar), which will need that clock seam anyway; add this test with it.
