# Web sidebar: pin, unpin, resize

## Intent

The desktop sidebar in `web/src/components/Shell.tsx` is a fixed `15.5rem` grid column. Users cannot
widen it, narrow it, or get it out of the way. Make it behave like a dockable drawer: pinned (docked,
resizable, pushes content) or unpinned (hidden, slides in over the content on demand). Build it from
maintained components, not hand-rolled drag or overlay logic.

## Goals

- **Pinned:** sidebar is a docked panel beside the content with a drag handle on its inner edge.
  Width is a percentage of the shell, not a fixed length: default 20%, min 15%, max 90%.
- **Unpinned:** the panel is removed and content takes the full width. A menu button in the header,
  or hovering the inline-start screen edge, opens the same sidebar over the content as a non-modal
  `Sheet`. The overlay opens at the last pinned percentage (of the viewport width).
- A Pin / PinOff toggle (lucide) in the sidebar header switches modes.
- Pin state and width persist per browser across reloads.
- Mobile (below `md`) keeps today's `Sheet` behavior unchanged.
- RTL keeps working: the panel, handle, edge trigger, and sheet sit on the inline-start side.

## Non-goals

- Resizing the unpinned overlay (agreed: resize is pinned-only).
- Collapsing to an icon rail.
- Syncing layout preferences to the server or across devices.

## Anti-goals

- Hand-written drag/resize math, pointer handling, or focus trapping.
- Hard-coded pixel widths for the sidebar.

## Approaches

- **A. shadcn `resizable` (`react-resizable-panels`) for the pinned panel + the existing Radix `Sheet`
  for unpinned and mobile.** Chosen. The library owns drag, keyboard resizing of the handle,
  percentage sizing with min/max, and layout persistence; Radix owns the overlay. Our code is the pin
  state, the edge-hover trigger, and passing the saved percentage to the sheet.
- **B. shadcn `sidebar` + `resizable`.** Rejected: its offcanvas mode pushes content rather than
  overlaying, and two systems would own the width.
- **C. dockview.** Rejected: an IDE docking layout engine for a single sidebar.

## Decided

- Percent sizing 15–90%, default 20% (the user set the range; the default is my call).
- Overlay width follows the last pinned percentage; no handle on the overlay.
- Add the shadcn `resizable` component to `src/components/ui/` via the shadcn CLI (pnpm), not by hand.

## Left to the builder

- Persistence mechanism: the library's own layout storage if it covers the panel size; pin state in
  `localStorage` behind a small hook, wrapped in try/catch. Default to pinned when nothing is saved.
- Edge-hover trigger details (strip width, open delay) and whether the sheet closes on pointer leave
  or only on outside click / Escape / navigation. Navigation closes it, as on mobile today.
- Accessible names for the pin toggle and resize handle go through i18n (`shell.*` keys).
- Tests: Shell tests for pin toggle, persisted state, unpinned open/close; the library's drag itself
  is not retested.
