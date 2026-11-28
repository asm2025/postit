# D4a verification (human checks)

Checks only a person can do, because they need the Zitadel passwords, a signed-in browser, or the dev
stack running with the mock platforms. Each entry says why an agent cannot do it and when it is due.
`2026-11-27-consolidated-verification.md` has no D4a-triggered items: its D2 checks wait for D4c and
its Papercut check for D4d.

## 1. Run the D4a end-to-end spec

- **Item:** from `web/`, with the dev stack up (`cargo dev` from `backend/` so the mock platforms exist,
  `pnpm dev`), run `POSTIT_E2E=1 POSTIT_ADMIN_PASSWORD=... POSTIT_MEMBER_PASSWORD=... pnpm exec playwright test e2e/d4a.spec.ts`.
  The spec walks: the admin switches the mock platforms on; the owner connects an account by credentials
  and one through the mock OAuth sign-in, imports a 3-row file (check, then connect), and makes a target
  group; the owner gives `member@postit.com` publish access to one account; the member switches to that
  workspace and sees only that account, no Connect button, and the banner; the owner revokes and the
  member's tab returns to their own workspace with the notice.
- **Why an agent cannot do it:** it signs in through Zitadel, whose passwords sit in the maintainer's vault
  and are not given to an agent.
- **Trigger:** before merging D4a. The spec was written and parsed (`playwright test --list`) but never
  run; the mock OAuth hop and the selectors are the likeliest places to need a fix.
- **Status 2026-11-28 (open, not run):** selectors checked against `web/src` and the mock
  descriptor; every label and role name the spec uses exists. The run is blocked: `mock_oauth` is
  registered only through the test seam `start_with_agents` (`backend/crates/server/src/compose.rs`
  registers `mock` and `mock_direct` only), so neither `cargo dev` nor the development image offers
  "Mock OAuth" and the spec fails at its first test (`Offer Mock OAuth to people`) and again at the
  OAuth connect. The spec is also not re-runnable on one database (a second run finds `e2e-shop`
  and `e2e-a..c` already connected): run it on an emptied database (`./db.sh drop development`),
  with `member@postit.com` signed in once and approved.

## 2. Walk the D4a flow by hand in a browser

- **Item:** on `https://postit.local:44315`, sign in as the owner and walk the plan's "done when" flow:
  connect an account by credentials (guide steps shown, discovery checklist, confirm) and through the mock
  OAuth sign-in (the address bar shows no `connect_id` after the return); import a file with a few rows and
  watch the progress bar and report; try a file past the size limit and check the "stopped" choices
  (connect the rows read, cancel); then sign in as the delegate in a second profile and act in the owner's
  workspace (banner, hidden owner actions, switching back). Also click a Web Push notification and check
  it lands on the right page.
- **Why an agent cannot do it:** the Playwright browser tools need the maintainer to sign in, and the
  delegate's session needs a second sign-in; a real Web Push click needs a browser that granted
  notifications.
- **Trigger:** before merging D4a, after item 1.
- **Status 2026-11-28 (open, partly done):** walked in Chrome on the Docker `-App` stack with
  `pnpm dev`. Passed: Mock switch on `/admin/platforms`; connect by credentials (guide step,
  checklist, confirm, "1 account connected." status); 3-row import, check then connect (all three
  connected, 17 jobs succeeded); progress bar ("Rows checked" up to 1,000); a 1,050-row file stopped
  at line 1001 with the row-limit reason, the connect run waited with "Connect the 1000 rows read"
  and "Cancel import", and cancel confirmed then reported "Import canceled. Nothing was connected.";
  target group with two accounts; grant to `member@postit.com` (Publish, only `e2e-shop`, "1
  account"); the member's switcher lists "Postit Admin's workspace · Can publish · 1 account" and
  the banner shows with "Back to my workspace". Not done: the mock OAuth connect (no `mock_oauth`
  in dev builds, see item 1); the 5 MiB byte limit (only the 1,000-row limit was tried); the
  member's accounts view (only `e2e-shop`, no Connect or Import), owner-only pages, switching back,
  and the revoke notice; the Web Push click. Defects found: (a) a committed import's report still
  labels every connected row "Ready to connect" (`importStatus.ok` serves the check and the connect
  run alike), and nothing says the rows were connected; (b) "Connect the 1000 rows read" does not
  format the count ("1,000" elsewhere); (c) in the sidebar the workspace switcher overflows the
  15.5rem column with a long label (`SelectTrigger` is `w-fit whitespace-nowrap h-11`).
