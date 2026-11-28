# Web sidebar pin and resize Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the fixed desktop sidebar into a drawer that can be pinned (docked, drag-resizable in percent) or unpinned (hidden, opened over the content).

**Architecture:** The shell is always one shadcn `resizable` panel group (`react-resizable-panels` v4). The content panel is always rendered and never remounts; the sidebar panel and its handle are rendered only when docked (desktop and pinned). Unpinned desktop and mobile open the existing Radix `Sheet` instead: non-modal on desktop at the last pinned percentage, modal on mobile as today. The library persists the docked layout; pin state is one `localStorage` flag behind a hook.

**Tech Stack:** React 19, `react-resizable-panels` 4.24.1 via shadcn `resizable`, Radix Dialog (`Sheet`), lucide-react, Tailwind 4, vitest + Testing Library.

**Spec:** `docs/specs/2026-11-28-web-sidebar-pin-resize-design.md`

## Global Constraints

- Sidebar width is a percentage: default `20%`, min `15%`, max `90%`. No pixel widths for the sidebar.
- Resize only when pinned; the overlay has no handle and opens at the last pinned percentage of the viewport (`vw`).
- Mobile (below Tailwind `md`, `48rem`) keeps today's modal `Sheet` unchanged. Note: today the sheet's own `data-[side=inline-start]:w-3/4` and `sm:max-w-sm` win over the `w-72` that `Shell.tsx` passes (higher specificity), so the mobile sheet is really 75% wide, capped at 24rem. Keep that; drop the ineffective `w-72`.
- Inline-start placement everywhere (RTL safe): use logical classes (`start-0`, `border-e`, `ms-*`), never `left`/`right`.
- pnpm only, run from PowerShell (the Git Bash pnpm shim is broken). Exact version pins in `package.json`, as the file does today.
- `react-resizable-panels` v4 treats a bare number as pixels: every size is a string with `%` (`"20%"`).
- The library's `Group` sets `display:flex`, `width:100%`, `height:100%`, and `overflow:hidden` inline, after any class. Classes on the group cannot hide it or size it: decide docked or not in JS (`useIsDesktop() && pinned`) and put sizing classes on a wrapper `div`. A `Panel`'s `className` lands on its inner scrolling `div` (`overflow:auto`).
- One tree for every mode. Pinning, unpinning, or crossing `md` (a window snapped to half a 1366px screen, a phone rotated to landscape) must not remount the `<Outlet>`, or unsaved form state is lost.
- The library stores the layout under `react-resizable-panels:<id>` as JSON (`{"sidebar":20,"content":80}`), and it saves every layout commit, including ones it makes itself. It `JSON.parse`s the stored value during render without a try/catch.
- No new copy outside the i18n catalog; American English.
- Every `localStorage` read and write is wrapped in try/catch, and the `localStorage` property itself is read inside the try (it throws when site data is blocked). A missing, throwing, or corrupt storage falls back to the defaults (pinned, `20%`).
- Quality gate from `web/`: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`.

## Carried in

- No `docs/plans/*-deferred.md` entry matches this work.
- `docs/plans/2026-11-27-d4a-workspace-accounts-verification.md` item (c), the workspace switcher overflowing the 15.5rem sidebar column, touches this sidebar. Commit 53e72103 looks like the fix; remind the user it is waiting for their confirmation. Do not mark it resolved yourself.

## Review Focus

- Storage that throws (private window, blocked site data), including the `localStorage` getter: the shell renders pinned at 20%, not crash. Test in Task 1.
- A corrupt saved layout (`'{bad'`): `safeStorage` returns `null`, so the library's unguarded `JSON.parse` never sees it. Tests in Tasks 1 and 3.
- A saved layout outside 15–90% (older build, hand-edited storage): the library clamps the docked panel to min/max; the overlay width and the sidebar panel's `defaultSize` go through `clampPercent`. Tests in Tasks 1 and 3.
- Removing the sidebar panel (unpinning, going mobile) must not save `{"content":100}` over the saved width. Test in Task 3.
- Page state survives pin, unpin, and the `md` breakpoint (one tree, content panel keyed). Test in Task 3.
- Focus: unpinning moves focus to the menu button; pinning from the overlay moves it to the docked "Unpin sidebar" button; hovering the edge does not take focus from the page. Tests in Task 3.
- A sheet open on mobile closes when the window widens to desktop while pinned (`open && !docked`). Test in Task 3.
- Navigating from the unpinned overlay closes it, as the mobile sheet does today. Test in Task 3.
- The overlay width is applied in a real browser (jsdom loads no CSS, so tests only see the custom property). Checked by hand in Task 3 step 6.

---

### Task 1: Sidebar layout state

**Files:**
- Create: `web/src/components/shell/sidebarLayout.ts`
- Test: `web/src/components/shell/sidebarLayout.test.ts`
- Modify: `web/src/test/setup.ts` (clear the two keys after every test)

**Interfaces:**
- Produces:
  - `SIDEBAR_PANEL_ID = 'sidebar'`, `CONTENT_PANEL_ID = 'content'`, `SIDEBAR_LAYOUT_ID = 'postit.sidebar.layout'` (passed to `useDefaultLayout`), `SIDEBAR_LAYOUT_KEY = 'react-resizable-panels:postit.sidebar.layout'` (the key the library writes; tests and cleanup use it), `PINNED_KEY = 'postit.sidebar.pinned'`
  - `SIDEBAR_DEFAULT = 20`, `SIDEBAR_MIN = 15`, `SIDEBAR_MAX = 90` (percent numbers; callers append `%`)
  - `clampPercent(value: number | undefined): number`: `undefined`/`NaN` gives `SIDEBAR_DEFAULT`, otherwise clamps to `[SIDEBAR_MIN, SIDEBAR_MAX]`
  - `safeStorage: Pick<Storage, 'getItem' | 'setItem'>`: reads `window.localStorage` inside a try on every call; `getItem` returns `null` when storage throws or the value is not valid JSON; `setItem` does nothing when storage throws. Passed to the library's `useDefaultLayout`.
  - `useSidebarPinned(): [pinned: boolean, setPinned: (pinned: boolean) => void]`: reads `PINNED_KEY` through `safeStorage` (`'false'` means unpinned, anything else pinned), writes on set
  - `useIsDesktop(): boolean`: `useSyncExternalStore` over `matchMedia('(min-width: 48rem)')` and its `change` event; `true` when `matchMedia` is missing (jsdom)

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
  expect(localStorage.getItem(PINNED_KEY)).toBe('false')
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

it('stays pinned when the localStorage property itself throws', () => {
  vi.spyOn(window, 'localStorage', 'get').mockImplementation(() => { throw new DOMException('blocked', 'SecurityError') })
  expect(safeStorage.getItem('x')).toBeNull()
  expect(() => safeStorage.setItem('x', '1')).not.toThrow()
  expect(renderHook(() => useSidebarPinned()).result.current[0]).toBe(true)
})

it('hides a corrupt saved layout from the library', () => {
  localStorage.setItem(SIDEBAR_LAYOUT_KEY, '{bad')
  expect(safeStorage.getItem(SIDEBAR_LAYOUT_KEY)).toBeNull()
  localStorage.setItem(SIDEBAR_LAYOUT_KEY, '{"sidebar":30,"content":70}')
  expect(safeStorage.getItem(SIDEBAR_LAYOUT_KEY)).toBe('{"sidebar":30,"content":70}')
})
```

Restore mocks in the test file's `afterEach` (it runs before the setup file's, which clears storage). If jsdom does not let `vi.spyOn` replace the `localStorage` getter, override it with `Object.defineProperty(window, 'localStorage', { configurable: true, get })` and put the original descriptor back in `afterEach`.

In `src/test/setup.ts`, beside `localStorage.removeItem('postit.tzAsked')`, remove `PINNED_KEY` and `SIDEBAR_LAYOUT_KEY`. Every render of the shell writes the layout key, so the cleanup belongs in the global hook, not in each test file.

- [ ] **Step 2: Run to verify they fail**

Run (PowerShell, `web/`): `pnpm exec vitest run src/components/shell/sidebarLayout.test.ts`
Expected: FAIL, module not found.

- [ ] **Step 3: Implement `web/src/components/shell/sidebarLayout.ts`** with the exports above. `useSidebarPinned` keeps React state initialized from `safeStorage` and writes through it.

- [ ] **Step 4: Run to verify they pass** (same command). Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/shell/sidebarLayout.ts web/src/components/shell/sidebarLayout.test.ts web/src/test/setup.ts
git commit -m "feat(web): sidebar pin and width state"
```

### Task 2: Add the shadcn `resizable` component

**Files:**
- Create: `web/src/components/ui/resizable.tsx` (generated)
- Modify: `web/package.json`, `web/pnpm-lock.yaml`

**Interfaces:**
- Produces: `ResizablePanelGroup`, `ResizablePanel`, `ResizableHandle` from `@/components/ui/resizable`, wrapping `Group`, `Panel`, `Separator` of `react-resizable-panels`.

- [ ] **Step 1:** From `web/` in PowerShell, add the pinned dependency first, so the shadcn CLI does not pull the newest release (4.26.0 is one day old at the time of writing): `pnpm add -E react-resizable-panels@4.24.1`. Then `pnpm exec shadcn add resizable`. If the CLI rewrote the version in `package.json`, put back the exact `4.24.1` and run `pnpm install`.
- [ ] **Step 2:** Check the generated file matches the repo's shadcn style: import `cn` from `'cn'` as `sheet.tsx` does (the CLI writes `@/lib/utils`, which re-exports it; change it for consistency). Check it uses the v4 names (`Group`, `Panel`, `Separator`, `orientation`). Keep the generated code otherwise as is.
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
- Consumes: everything Task 1 produces; Task 2's `Resizable*` components; `useDefaultLayout({ id, storage })` from `react-resizable-panels` (returns `defaultLayout`, a map of panel id to percent, and `onLayoutChanged`).
- Produces: `Sidebar` gains an optional `pin?: { pinned: boolean; onToggle: () => void }`; when given, a pin button renders at the end of the logo row (`ms-auto`).

New i18n keys in `shell`: `"pin": "Pin sidebar"`, `"unpin": "Unpin sidebar"`, `"resize": "Resize sidebar"`.

Behavior to build:

- **State.** `isDesktop = useIsDesktop()`, `[pinned, setPinned] = useSidebarPinned()`, `docked = isDesktop && pinned`. `{ defaultLayout, onLayoutChanged } = useDefaultLayout({ id: SIDEBAR_LAYOUT_ID, storage: safeStorage })`. `saved = clampPercent(defaultLayout?.[SIDEBAR_PANEL_ID])`.
- **One tree.** A wrapper `div` with `flex h-dvh` (`dvh`, not `vh`, so mobile browser bars do not cut off the bottom) holds one `ResizablePanelGroup orientation="horizontal"` and the `Sheet`. Inside the group:
  - When `docked`: `ResizablePanel key="sidebar" id="sidebar" defaultSize={`${saved}%`} minSize="15%" maxSize="90%"` holding the `<aside>` (keep its padding and `border-e`; drop the grid, `sticky`, and `h-screen`), then `ResizableHandle key="handle" aria-label={t('shell.resize')}`.
  - Always: `ResizablePanel key="content" id="content" className="flex min-w-0 flex-col"` holding the header, banner, and main. It is the page's scroll container now (the library gives the inner `div` `overflow:auto`), in place of the window.
  - Pass `defaultLayout={docked ? defaultLayout : undefined}`: the library applies it only on mount, and its panel ids must match the panels rendered then. A sidebar panel added later starts at its `defaultSize`.
  - Pass `onLayoutChanged={(layout, meta) => { if (SIDEBAR_PANEL_ID in layout) onLayoutChanged(layout, meta) }}`, so removing the sidebar panel never saves `{"content":100}` over the saved width.
- **Header.** The menu button renders only when `!docked` (mobile, or unpinned desktop), with a ref. Drop `md:justify-end` and `md:ms-0`; the actions keep `ms-auto` on every width, so the menu button stays at the start.
- **Edge strip.** When `isDesktop && !pinned`: `div` with `fixed inset-y-0 start-0 z-40 w-2`, `data-testid="sidebar-edge"`, opening the sheet on `pointerenter` (no delay, no timer) and marking the open as hover-started.
- **Sheet.** One `Sheet` for mobile and unpinned desktop: `open={open && !docked}` (a mobile sheet left open closes when the window widens while pinned), `modal={!isDesktop}`, `showCloseButton={!isDesktop}` (on desktop the overlay closes on Escape, outside click, navigation, or pinning, and the close button would sit on top of the pin button). `SheetContent` gets `side="inline-start"`, `style={{ '--sidebar-overlay': `${saved}vw` } as React.CSSProperties}`, and class `p-4 pt-6 md:w-(--sidebar-overlay)! md:max-w-none!`. The `!` (important) is required: the sheet's `data-[side=inline-start]:w-3/4` and `data-[side=inline-start]:sm:max-w-sm` are more specific than plain utilities. `onOpenAutoFocus` calls `preventDefault()` when the open was hover-started, so grazing the edge never takes focus from the page; a menu-button open keeps the default focus move. Navigation closes the sheet (existing `onNavigate`).
- **Pin button.** Rendered only when `isDesktop` (JS, not a CSS class): lucide `PinOff` with `aria-label={t('shell.unpin')}` when pinned, `Pin` with `aria-label={t('shell.pin')}` when unpinned, icons `aria-hidden`.
  - Unpinning sets pinned false, then focuses the menu button after the render (the focused pin button is gone).
  - Pinning from the overlay sets pinned true and closes the sheet; `onCloseAutoFocus` calls `preventDefault()` for this close, and focus moves to the docked "Unpin sidebar" button after the render (the menu button it would return to is gone).

- [ ] **Step 1: Write the failing tests** in `Shell.test.tsx`. In jsdom `useIsDesktop()` is `true` (no `matchMedia`). The setup file clears the sidebar keys. Add a `page` route variant whose element is `<input aria-label="Draft" />`, and a `mobile()` helper that defines `window.matchMedia` (`Object.defineProperty`, `configurable: true`) returning a controllable query (`matches`, `addEventListener`, `removeEventListener`, and a way to flip `matches` and fire `change`); remove it with `Reflect.deleteProperty(window, 'matchMedia')` in `afterEach`. Do not use `vi.unstubAllGlobals`: it would also remove the setup file's `ResizeObserver` stub.

```tsx
it('pins by default with a resize handle', async () => {
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Unpin sidebar' })).toBeInTheDocument()
  expect(screen.queryByRole('button', { name: 'Open menu' })).not.toBeInTheDocument()
})

it('unpins, remembers it, and opens over the content from the edge', async () => {
  mock('member')
  const view = renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Unpin sidebar' }))
  expect(screen.queryByRole('separator', { name: 'Resize sidebar' })).not.toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Open menu' })).toHaveFocus()
  expect(localStorage.getItem(PINNED_KEY)).toBe('false')
  await userEvent.hover(screen.getByTestId('sidebar-edge'))
  const sheet = await screen.findByRole('dialog')
  expect(within(sheet).getByRole('link', { name: /home/i })).toBeInTheDocument()
  view.unmount()
  renderApp(null, { routes: app() })
  expect(await screen.findByTestId('sidebar-edge')).toBeInTheDocument()
})

it('hovering the edge leaves focus on the page', async () => {
  localStorage.setItem(PINNED_KEY, 'false')
  mock('member')
  renderApp(null, { routes: app(<input aria-label="Draft" />) })
  const draft = await screen.findByRole('textbox', { name: 'Draft' })
  await userEvent.click(draft)
  await userEvent.hover(screen.getByTestId('sidebar-edge'))
  await screen.findByRole('dialog')
  expect(draft).toHaveFocus()
})

it('pinning from the overlay closes it, docks the sidebar, and keeps focus nearby', async () => {
  localStorage.setItem(PINNED_KEY, 'false')
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Pin sidebar' }))
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  expect(screen.getByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Unpin sidebar' })).toHaveFocus()
})

it('keeps the page state across unpin and pin', async () => {
  mock('member')
  renderApp(null, { routes: app(<input aria-label="Draft" />) })
  await userEvent.type(await screen.findByRole('textbox', { name: 'Draft' }), 'hello')
  await userEvent.click(screen.getByRole('button', { name: 'Unpin sidebar' }))
  await userEvent.click(screen.getByRole('button', { name: 'Open menu' }))
  await userEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Pin sidebar' }))
  expect(screen.getByRole('textbox', { name: 'Draft' })).toHaveValue('hello')
})

it('closes the overlay on navigation', async () => {
  localStorage.setItem(PINNED_KEY, 'false')
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  await userEvent.click(within(await screen.findByRole('dialog')).getByRole('link', { name: /home/i }))
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
})

it('opens the overlay at the saved width, clamped', async () => {
  localStorage.setItem(PINNED_KEY, 'false')
  localStorage.setItem(SIDEBAR_LAYOUT_KEY, JSON.stringify({ sidebar: 95, content: 5 }))
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  expect((await screen.findByRole('dialog')).style.getPropertyValue('--sidebar-overlay')).toBe('90vw')
})

it('unpinning keeps the saved width', async () => {
  localStorage.setItem(SIDEBAR_LAYOUT_KEY, JSON.stringify({ sidebar: 33, content: 67 }))
  mock('member')
  renderApp(null, { routes: app() })
  await userEvent.click(await screen.findByRole('button', { name: 'Unpin sidebar' }))
  expect(JSON.parse(localStorage.getItem(SIDEBAR_LAYOUT_KEY) ?? '{}')).toMatchObject({ sidebar: 33 })
  await userEvent.click(screen.getByRole('button', { name: 'Open menu' }))
  expect((await screen.findByRole('dialog')).style.getPropertyValue('--sidebar-overlay')).toBe('33vw')
})

it('renders pinned when the saved layout is corrupt', async () => {
  localStorage.setItem(SIDEBAR_LAYOUT_KEY, '{bad')
  mock('member')
  renderApp(null, { routes: app() })
  expect(await screen.findByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
})

it('keeps the modal sheet on mobile, and closes it when the window widens', async () => {
  const query = mobile()
  mock('member')
  renderApp(null, { routes: app() })
  expect(screen.queryByRole('separator', { name: 'Resize sidebar' })).not.toBeInTheDocument()
  await userEvent.click(await screen.findByRole('button', { name: 'Open menu' }))
  const sheet = await screen.findByRole('dialog')
  expect(within(sheet).queryByRole('button', { name: /pin sidebar/i })).not.toBeInTheDocument()
  expect(within(sheet).getByRole('button', { name: 'Close' })).toBeInTheDocument()
  act(() => query.set(true))
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument())
  expect(screen.getByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument()
})
```

`app()` takes an optional index element (default `<div>page body</div>`). Use the `act` that `@testing-library/react` exports.

Existing tests that find nav links with `getByRole('link', …)` must still pass: the docked panel renders the nav once on desktop, and the sheet is closed by default.

- [ ] **Step 2: Run to verify they fail**

Run: `pnpm exec vitest run src/components/Shell.test.tsx`
Expected: the new tests FAIL (no separator, no pin button); the existing ones pass.

- [ ] **Step 3: Implement** the behavior above in `Shell.tsx` and add the three i18n keys.

- [ ] **Step 4: Run to verify they pass** (same command). Expected: all PASS. If the library's drag code needs a browser API jsdom lacks, stub it in `src/test/setup.ts` beside the existing `ResizeObserver` stub, with a one-line comment saying why.

- [ ] **Step 5: Full web gate**

Run: `pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build`
Expected: all pass (including `kitGuard.test.ts` and the i18n catalog tests).

- [ ] **Step 6: Check in the browser** with `pnpm dev` (or the `run` skill) and the Playwright or Chrome tools:
  - Drag the handle: it stops at 15% and 90%, and the width persists across reload. Keyboard-resize the focused handle with the arrow keys.
  - Unpin, hover the edge, open from the menu button, pin from the overlay.
  - Measure the overlay (`getBoundingClientRect().width`) against the saved percentage of `innerWidth`, at a saved width above 24rem: it must match, not stop at 75% or 24rem.
  - On a long page, the content panel scrolls and the sidebar does not move; a page with `DangerZone` keeps it stuck to the bottom.
  - Type into a form, then narrow the window below `md` and widen it again while pinned: the text stays, the menu button shows below `md`, and nothing is cut off.
  - Repeat once with `dir="rtl"` set on `<html>` in devtools: panel, handle, edge strip, and sheet sit on the right.

  Fix anything that fails in this task. Anything only a person can check (for example a real touch device) goes to `docs/plans/2026-11-28-web-sidebar-pin-resize-verification.md`.

- [ ] **Step 7: Commit**

```bash
git add web/src/components/Shell.tsx web/src/components/Shell.test.tsx web/src/i18n/en-US/common.json web/src/test/setup.ts
git commit -m "feat(web): pinnable, resizable sidebar"
```

## Finishing

- Fix every issue found, minor ones included. Only work that truly cannot be done in this plan goes to `docs/plans/2026-11-28-web-sidebar-pin-resize-deferred.md`, and human-only checks go to `docs/plans/2026-11-28-web-sidebar-pin-resize-verification.md`. Commit either file with the branch and name it in the final report.
- Remind the user of the d4a verification item (c) listed under "Carried in".
