# Web sidebar pin and resize Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the fixed desktop sidebar into a drawer that can be pinned (docked, drag-resizable in percent) or unpinned (hidden, opened over the content).

**Architecture:** Pinned mode renders the shell as a shadcn `resizable` panel group (`react-resizable-panels` v4) with the sidebar panel and the content panel; the library persists the layout. Unpinned mode drops the panel group and opens the existing Radix `Sheet`, non-modal on desktop, at the last pinned percentage. Pin state is one `localStorage` flag behind a hook.

**Tech Stack:** React 19, `react-resizable-panels` 4.x via shadcn `resizable`, Radix Dialog (`Sheet`), lucide-react, Tailwind 4, vitest + Testing Library.

**Spec:** `docs/specs/2026-11-28-web-sidebar-pin-resize-design.md`

## Global Constraints

- Sidebar width is a percentage: default `20%`, min `15%`, max `90%`. No pixel widths for the sidebar.
- Resize only when pinned; the overlay has no handle and opens at the last pinned percentage of the viewport (`vw`).
- Mobile (below Tailwind `md`, `48rem`) keeps today's modal `Sheet` at `w-72`.
- Inline-start placement everywhere (RTL safe): use logical classes (`start-0`, `border-e`, `ms-*`), never `left`/`right`.
- pnpm only, run from PowerShell (the Git Bash pnpm shim is broken). Exact version pins in `package.json`, as the file does today.
- `react-resizable-panels` v4 treats a bare number as pixels: every size is a string with `%` (`"20%"`).
- No new copy outside the i18n catalog; American English.
- Every `localStorage` read and write is wrapped in try/catch; a missing or throwing storage falls back to the defaults (pinned, `20%`).
- Quality gate from `web/`: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`.

## Review Focus

- Storage that throws (private window, blocked site data): the shell must render pinned at 20%, not crash. Test in Task 1.
- A saved layout outside 15–90% (older build, hand-edited storage): the library clamps to min/max; the overlay width must be clamped too. Test in Task 1 (`clampPercent`).
- Unpinning while a route has focus inside the sidebar, then pinning from inside the open overlay: the overlay closes and the docked panel appears; no stuck open sheet. Test in Task 3.
- Navigating from the unpinned overlay closes it, as the mobile sheet does today. Test in Task 3.
- Pinned on desktop then the window shrinks below `md`: the panel group is hidden by CSS and the mobile menu button shows; nothing is cut off. Covered by the `md:` classes; checked by hand in Task 3 step 6.

---

### Task 1: Sidebar layout state

**Files:**
- Create: `web/src/components/shell/sidebarLayout.ts`
- Test: `web/src/components/shell/sidebarLayout.test.ts`

**Interfaces:**
- Produces:
  - `SIDEBAR_PANEL_ID = 'sidebar'`, `CONTENT_PANEL_ID = 'content'`, `SIDEBAR_LAYOUT_ID = 'postit.sidebar.layout'`, `PINNED_KEY = 'postit.sidebar.pinned'`
  - `SIDEBAR_DEFAULT = 20`, `SIDEBAR_MIN = 15`, `SIDEBAR_MAX = 90` (percent numbers; callers append `%`)
  - `clampPercent(value: number | undefined): number` — `undefined`/`NaN` gives `SIDEBAR_DEFAULT`, otherwise clamps to `[SIDEBAR_MIN, SIDEBAR_MAX]`
  - `safeStorage: Pick<Storage, 'getItem' | 'setItem'>` — delegates to `localStorage`, returns `null` / does nothing when it throws; passed to the library's `useDefaultLayout`
  - `useSidebarPinned(): [pinned: boolean, setPinned: (pinned: boolean) => void]` — reads `PINNED_KEY` (`'false'` means unpinned, anything else pinned), writes on set
  - `useIsDesktop(): boolean` — `useSyncExternalStore` over `matchMedia('(min-width: 48rem)')`; `true` when `matchMedia` is missing (jsdom)

- [ ] **Step 1: Write the failing tests**

```ts
it('clamps the sidebar percentage', () => {
  expect(clampPercent(undefined)).toBe(20)
  expect(clampPercent(Number.NaN)).toBe(20)
  expect(clampPercent(5)).toBe(15)
  expect(clampPercent(95)).toBe(90)
  expect(clampPercent(33)).toBe(33)
})

it('defaults to pinned and remembers unpinning', () => {
  const { result, unmount } = renderHook(() => useSidebarPinned())
  expect(result.current[0]).toBe(true)
  act(() => result.current[1](false))
  expect(result.current[0]).toBe(false)
  expect(localStorage.getItem('postit.sidebar.pinned')).toBe('false')
  unmount()
  expect(renderHook(() => useSidebarPinned()).result.current[0]).toBe(false)
})

it('stays pinned when storage throws', () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('blocked') })
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('blocked') })
  const { result } = renderHook(() => useSidebarPinned())
  expect(result.current[0]).toBe(true)
  expect(() => act(() => result.current[1](false))).not.toThrow()
  expect(safeStorage.getItem('x')).toBeNull()
})
```

Clear `postit.sidebar.*` keys and restore mocks in `afterEach`.

- [ ] **Step 2: Run to verify they fail**

Run (PowerShell, `web/`): `pnpm exec vitest run src/components/shell/sidebarLayout.test.ts`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement `web/src/components/shell/sidebarLayout.ts`** with the exports above. `useSidebarPinned` keeps React state initialized from `safeStorage` and writes through it.

- [ ] **Step 4: Run to verify they pass** (same command). Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shell/sidebarLayout.ts web/src/components/shell/sidebarLayout.test.ts
git commit -m "feat(web): sidebar pin and width state"
```

### Task 2: Add the shadcn `resizable` component

**Files:**
- Create: `web/src/components/ui/resizable.tsx` (generated)
- Modify: `web/package.json`, `web/pnpm-lock.yaml`

**Interfaces:**
- Produces: `ResizablePanelGroup`, `ResizablePanel`, `ResizableHandle` from `@/components/ui/resizable`, wrapping `Group`, `Panel`, `Separator` of `react-resizable-panels`.

- [ ] **Step 1:** From `web/` in PowerShell: `pnpm exec shadcn add resizable`. Then pin `react-resizable-panels` in `package.json` to an exact version released at least a week before execution (4.24.1 at the time of writing) and run `pnpm install`.
- [ ] **Step 2:** Check the generated file matches the repo's shadcn style (imports `cn` from `'cn'` as `sheet.tsx` does; fix the import if the CLI wrote `@/lib/utils`). Keep the generated code otherwise as is.
- [ ] **Step 3:** Run `pnpm typecheck && pnpm lint && pnpm format:check`. Expected: all pass.
- [ ] **Step 4: Commit**

```bash
git add web/src/components/ui/resizable.tsx web/package.json web/pnpm-lock.yaml
git commit -m "feat(web): add shadcn resizable"
```

### Task 3: Pinned and unpinned shell

**Files:**
- Modify: `web/src/components/Shell.tsx` (the `Shell` and `Sidebar` components)
- Modify: `web/src/i18n/en-US/common.json` (`shell` block)
- Test: `web/src/components/Shell.test.tsx`

**Interfaces:**
- Consumes: everything Task 1 produces; Task 2's `Resizable*` components; `useDefaultLayout({ id, storage })` from `react-resizable-panels` (returns `defaultLayout` and `onLayoutChanged`, a map of panel id to percent).
- Produces: `Sidebar` gains an optional `pin?: { pinned: boolean; onToggle: () => void }`; when given, a pin button renders next to the logo.

New i18n keys in `shell`: `"pin": "Pin sidebar"`, `"unpin": "Unpin sidebar"`, `"resize": "Resize sidebar"`.

Behavior to build:
- Desktop pinned: `ResizablePanelGroup` (`orientation="horizontal"`, class `hidden md:flex h-screen`, `defaultLayout`/`onLayoutChanged` from `useDefaultLayout({ id: SIDEBAR_LAYOUT_ID, storage: safeStorage })`) with `ResizablePanel id="sidebar" defaultSize="20%" minSize="15%" maxSize="90%"` holding the `<aside>` (keep its padding and `border-e`; drop the grid), a `ResizableHandle` with `aria-label={t('shell.resize')}`, and `ResizablePanel id="content"` holding the header, banner, and main. Below `md` the content renders without the group, as today.
- Desktop unpinned: no panel group. The menu button in the header shows on desktop too (drop `md:hidden` when unpinned). A fixed strip, `hidden md:block fixed inset-y-0 start-0 w-2 z-40`, `data-testid="sidebar-edge"`, opens the sheet on `pointerenter` (no delay, no timer).
- One `Sheet` for mobile and unpinned desktop: `modal={!isDesktop}`. `SheetContent` gets `style={{ '--sidebar-overlay': `${clampPercent(defaultLayout?.[SIDEBAR_PANEL_ID])}vw` }}` and class `w-72 md:w-[var(--sidebar-overlay)] sm:max-w-none`. Navigation closes it (existing `onNavigate`).
- Pin button: shown only on desktop (`hidden md:flex`), lucide `PinOff` with `aria-label={t('shell.unpin')}` when pinned, `Pin` with `aria-label={t('shell.pin')}` when unpinned. Pinning from inside the overlay sets pinned and closes the sheet.

- [ ] **Step 1: Write the failing tests** in `Shell.test.tsx` (jsdom: `useIsDesktop()` is `true`; clear `postit.sidebar.*` in `afterEach`):

```tsx
it('pins by default with a resize handle', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Unpin sidebar' })).toBeInTheDocument()
})

it('unpins, remembers it, and opens over the content from the edge', async () => {
  mock('member')
  const view = renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Unpin sidebar' }))
  expect(screen.queryByRole('separator', { name: 'Resize sidebar' })).not.toBeInTheDocument()
  expect(localStorage.getItem('postit.sidebar.pinned')).toBe('false')
  await userEvent.hover(screen.getByTestId('sidebar-edge'))
  const sheet = await screen.findByRole('dialog')
  expect(within(sheet).getByRole('link', { name: /home/i })).toBeInTheDocument()
  view.unmount()
  renderApp(null, { routes: app() })
  expect(await screen.findByTestId('sidebar-edge')).toBeInTheDocument()
})

it('pinning from the overlay closes it and docks the sidebar', async () => {
  localStorage.setItem('postit.sidebar.pinned', 'false')
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Pin sidebar' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  expect(screen.getByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
})

it('closes the overlay on navigation', async () => {
  localStorage.setItem('postit.sidebar.pinned', 'false')
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  await userEvent.click(within(await screen.findByRole('dialog')).getByRole('link', { name: /home/i }))
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
})

it('opens the overlay at the saved width, clamped', async () => {
  localStorage.setItem('postit.sidebar.pinned', 'false')
  localStorage.setItem('postit.sidebar.layout' /* key format per useDefaultLayout */, '...95% sidebar...')
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  expect((await screen.findByRole('dialog')).style.getPropertyValue('--sidebar-overlay')).toBe('90vw')
})
```

For the last test, write the stored layout in the exact key and value format `useDefaultLayout` uses (read it from the library source in `node_modules/react-resizable-panels`), with the sidebar at 95.

Existing tests that find nav links with `getByRole('link', …)` must still pass: the pinned panel renders the nav once on desktop, and the sheet is closed by default.

- [ ] **Step 2: Run to verify they fail**

Run: `pnpm exec vitest run src/components/Shell.test.tsx`
Expected: the five new tests FAIL (no separator, no pin button); the existing ones pass.

- [ ] **Step 3: Implement** the behavior above in `Shell.tsx` and add the three i18n keys.

- [ ] **Step 4: Run to verify they pass** (same command). Expected: all PASS. If the library's drag code needs a browser API jsdom lacks, stub it in `src/test/setup.ts` beside the existing `ResizeObserver` stub, with a one-line comment saying why.

- [ ] **Step 5: Full web gate**

Run: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`
Expected: all pass (including `kitGuard.test.ts` and the i18n catalog tests).

- [ ] **Step 6: Check in the browser** with `pnpm dev` (or the `run` skill): drag the handle (stops at 15% and 90%; persists across reload), keyboard-resize the focused handle with arrow keys, unpin, hover the edge, open from the menu button, pin from the overlay, shrink below `md` while pinned, and repeat once with `dir="rtl"` set on `<html>` in devtools. Record anything that fails as a fix in this task.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/Shell.tsx web/src/components/Shell.test.tsx web/src/i18n/en-US/common.json web/src/test/setup.ts
git commit -m "feat(web): pinnable, resizable sidebar"
```
