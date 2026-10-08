# P7.1 Web Design Polish and Themes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox syntax.

**Goal:** Restyle `web/` to the `!ref/design/Home/index.html` mockup: tokens, fonts, brand, sidebar/Home layout, segmented theme control.

**Architecture:** Mockup palette becomes CSS variables (`--bg`, `--surface`, ... ) on `:root` (light) and `.dark`; mapped onto shadcn variables so stock components follow. Fonts self-hosted via pinned `@fontsource-variable` packages. Layout components rewritten in Tailwind using the tokens.

**Tech Stack:** React 19, Tailwind 4, shadcn/base-ui, Vitest + Testing Library. JS tooling via pnpm in PowerShell only.

**Spec:** `!ref/plans/02. foundation.md` section P7.1; mockup `!ref/design/Home/index.html`.

## Global Constraints
- CSP is `font-src 'self'`: no Google Fonts, no remote assets.
- Accent pink `#e0679f`; focus rings pink; touch targets >= 44px; reduced motion respected; usable at 360px.
- Theme key `localStorage` `postit.theme`, default `system`, storage access wrapped in try/catch; `public/theme-init.js` applies before paint.
- Pinned exact versions (no `^`). Only SVGs ship in `web/public/`.
- Gates: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`.

## Review Focus
- localStorage throws (blocked): theme still works, falls back to system (test).
- Invalid stored value (`"purple"`): treated as system (test).
- System theme follows live OS change (existing behavior, keep).
- Pending count 0 / undefined: no badge, no "NaN".
- Long display names/emails truncate in account card, no overflow at 360px.

### Task 1: Tokens and fonts
Files: modify `web/package.json`, `web/src/index.css`, `web/index.html`.
- [ ] Add fonts: `pnpm add -E @fontsource-variable/bricolage-grotesque @fontsource-variable/onest @fontsource-variable/jetbrains-mono`; `pnpm remove @fontsource-variable/geist`.
- [ ] index.css: import the three fonts; `--font-sans` Onest, `--font-heading`/`--font-display` Bricolage, `--font-mono` JetBrains Mono; define mockup variables for `:root` (light) and `.dark`; map shadcn vars (`--background:var(--bg)`, `--card:var(--surface)`, `--primary:var(--pink)`, `--primary-foreground:var(--on-pink)`, `--muted:var(--raised)`, `--muted-foreground:var(--muted-text)`, `--border:var(--line)`, `--ring:var(--pink)`, `--accent:var(--raised)`...). Expose Tailwind colors `surface, raised, line, faint, pink, pink-text, pink-soft, on-pink, warn-*, track, disabled`. Mockup `--muted` text color is renamed `--muted-text` to avoid colliding with shadcn `--muted`.
- [ ] Reduced-motion rule; global pink focus-visible outline.
- [ ] Verify: `pnpm build`.

### Task 2: Brand assets
Files: create `web/public/postit-f.svg`, `postit-sb.svg`, `postit.svg`; modify `web/public/favicon.svg` (replace with badge), `web/index.html`; create `web/src/components/Logo.tsx`.
- [ ] Copy SVGs from `!ref/assets/`. `Logo` inlines the wordmark paths with ink `currentColor`-driven fill (`var(--ink-logo)`) and pink accent.
- [ ] favicon -> postit-sb.svg content.

### Task 3: Theme control (TDD)
Files: modify `ThemeProvider.tsx`; create `web/src/theme/ThemeToggle.tsx`, `ThemeToggle.test.tsx`; modify `public/theme-init.js`.
- [ ] Tests: renders three `aria-pressed` buttons; click sets `postit.theme` and `dark` class; storage that throws -> still switches, default system; invalid stored value -> system pressed.
- [ ] Implement `ThemeToggle`; validate stored values in theme-init.js (already restricted via ThemeProvider read; make init match).

### Task 4: Shell
Files: modify `Shell.tsx`, `Shell.test.tsx`.
- [ ] Sidebar: Logo, Main nav (Home, Profile with icons), "Admin" group (Users + pending count, Audit), env indicator, account card with sign-out icon button (aria-label "Sign out"). Drawer on narrow screens. Theme toggle moves to page header area (Home header + shell top bar on narrow).
- [ ] Update tests (nav links, badge count, members hidden).

### Task 5: Home
Files: modify `HomePage.tsx`, `AdminPanels.tsx`, `AccountCard.tsx`, `AuditList.tsx`, `HomePage.test.tsx`.
- [ ] Date line + greeting header with theme toggle; warn banner; approval queue (88px count, CTA); team summary (count, bar, legend); recent activity timeline; account card. Keep existing data hooks and approve/reject behavior (members: account card only).

### Task 6: Other screens
Files: `LoginPage.tsx`, `StatusScreens.tsx`, `ProfilePage.tsx`, `UsersPage.tsx`, `AuditPage.tsx`, ui `button.tsx`/`card.tsx`/`badge.tsx`/`input.tsx`/`select.tsx`.
- [ ] Card radius 20px, surface border; button heights >= 44px for default/lg; pill badges; headings use display font; page headings with mono eyebrow label.

### Task 7: Verify
- [ ] Gates pass. Grep for `fonts.googleapis`, `http` remote assets: none. Commit.
