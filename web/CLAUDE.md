# web/

Web client (from `web/`, Node per `.nvmrc`, pnpm only — never npm):

```sh
pnpm install
pnpm dev            # https://postit.local:44315 (dev certificate required)
pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build
```

Pages build from the shared kit in `src/components/kit/` (`DataTable`, `TablePagination`, `PageHeader`, `OptionSelect`, `DangerZone`, …) over the shadcn/Radix primitives in `src/components/ui/`; list filters and paging live in the URL through `useListParams`. `kitGuard.test.ts` fails when a page renders its own `<table>` or `<select>`. Add or extend a kit component rather than copying markup into a page.
