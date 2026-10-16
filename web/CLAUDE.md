# web/

Web client (from `web/`, Node per `.nvmrc`, pnpm only — never npm):

```sh
pnpm install
pnpm dev            # https://postit.local:44315 (dev certificate required)
pnpm typecheck && pnpm lint && pnpm format:check && pnpm test && pnpm build
```
