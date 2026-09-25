# postit

A self-hosted social publishing service for a small team. See `!ref/plans/` for the
implementation plans; `01. vision and architecture.md` first.

## Quick start (Windows)

Requires Docker Desktop, Rust 1.98.1, and Git for Windows (bundles `openssl`).

1. Generate and trust the dev certificate:

   ```powershell
   ./cert.ps1
   ```

   Then add this to `C:\Windows\System32\drivers\etc\hosts` as Administrator:

   ```
   127.0.0.1 postit.local
   ```

2. Bring up Postgres, Mailpit, Zitadel, and the TLS front door:

   ```powershell
   docker compose -f compose.yaml -f compose.dev.yaml up -d
   ```

   Zitadel's first-run setup takes 15-30 seconds. Watch it with
   `docker compose logs -f zitadel`.

3. Bootstrap the Zitadel project, OIDC app, and `member@postit.local` user, and write
   `server/config/local.toml`:

   ```powershell
   cd server
   cargo xtask zitadel-bootstrap
   ```

   This is idempotent — re-run it any time after `docker compose down` and back `up`.

4. Sign in at `https://postit.local:44330/ui/console`:

   - `admin@postit.local` / `PostitDev1!`
   - `member@postit.local` / `PostitDev1!`

   Zitadel's own emails (verification, password reset) land in Mailpit at
   `https://postit.local:44320`.

5. `nginx-app` (the API/worker/web placeholders on 44300/44305/44310) is behind the
   `app` compose profile:

   ```powershell
   docker compose -f compose.yaml -f compose.dev.yaml --profile app up -d
   ```

   These serve a placeholder page until plan 02 phases P6 (API/worker) and P7 (web)
   replace them.

All dev-only secrets in `compose.dev.yaml`, `docker/zitadel/steps.yaml`, and
`server/config/development.toml` are fixed, insecure, and clearly marked as such — never
reused outside development.

## Quick start (Linux / macOS)

Same steps, with `cert.sh` instead of `cert.ps1` and `/etc/hosts` instead of the Windows
hosts file (`cert.sh` prints the exact trust command for your OS). `cargo xtask
zitadel-bootstrap` is identical.

## Everyday commands

```sh
cargo build                     # dev build
cargo test --workspace          # all tests
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
docker compose -f compose.yaml -f compose.dev.yaml down     # stop the dev stack
```
