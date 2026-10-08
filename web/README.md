# postit web

The React client for postit: sign in through the OIDC provider, then manage users and read the audit log (admins), or see your account (members).

Stack: React 19, TypeScript (strict), Vite, Tailwind CSS v4 with shadcn/ui, React Router, TanStack Query, `openapi-fetch`, `oidc-client-ts` via `react-oidc-context`. Tests: Vitest, Testing Library, MSW; Playwright for an opt-in smoke test.

## Scripts

| Script                         | What it does                                                                |
| ------------------------------ | --------------------------------------------------------------------------- |
| `pnpm dev`                     | Vite dev server on `https://postit.local:44315` (needs the dev certificate) |
| `pnpm build`                   | Type-check and build to `dist/`                                             |
| `pnpm typecheck`               | `tsc --noEmit`                                                              |
| `pnpm lint`                    | ESLint (flat config, type-checked strict rules)                             |
| `pnpm format` / `format:check` | Prettier                                                                    |
| `ppnpm test`                   | Vitest                                                                      |
| `pnpm e2e`                     | Playwright smoke; set `POSTIT_E2E=1` and the stack must be running          |

## Structure

- `src/config`: runtime config (`/config.json`) and the auth config from the API.
- `src/api`: generated `schema.d.ts` (never edit by hand; `cargo xtask openapi` writes it), the `openapi-fetch` client, the auth-aware fetch, error mapping, the query client.
- `src/auth`: user manager (tokens in memory only), token source, session provider, silent-frame handling.
- `src/features/{me,users,audit,home}`: one folder per area.
- `src/components`: shell, dialogs, pager, and `ui/` (shadcn).
- `src/theme`, `src/lib`, `src/test`: theme, helpers, test setup and fixtures.
- `e2e`: Playwright smoke test.

Run `pnpm install` here before `cargo xtask openapi`.

## TypeScript 7

`tsc` is TypeScript 7 (the native compiler), which has no classic JS API. typescript-eslint and openapi-typescript still need that API, so `.pnpmfile.cjs` gives them `@typescript/typescript6` as a direct dependency in place of their `typescript` peer. Drop an entry from the hook once its package supports TypeScript 7.
