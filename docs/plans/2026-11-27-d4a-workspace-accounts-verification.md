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
