# Operator scripts: human verification

Checks for `docs/plans/2026-11-07-operator-scripts.md` that need the maintainer: the vault
password, a real terminal, or a judgment about the live machine. The automated suite
(`bash tests/scripts/run.sh`) pipes the vault password and every confirmation through
standard input, so the terminal side of these paths (no echo, how the prompts read) was not
exercised by an agent.

## 1. Re-pack the real vault with `vault pack`

- **Item:** run `./vault.ps1 pack` (or `./vault.sh pack`) once and type the current vault
  password at the script's `Vault password:` prompt (not echoed; the script checks it opens
  the current archive before packing). Then `./vault.ps1 check` with the same password (exit
  0, "the archive matches"), and commit `!ref/vault.7z`. This also re-packs the VAPID keys and database passwords rotated
  on 2026-11-07, which the D1 verification entry for VAPID waits on.
- **Why an agent cannot:** the vault password is the maintainer's, and the repository rules
  say Claude never re-packs the archive.
- **Trigger:** now, before the next push of `stage`.
- **Note:** the new archive stores every file under a `vault/` folder, so `7z x vault.7z`
  from `!ref/` and `vault unpack` both produce `!ref/vault/<environment>/`. `unpack` and
  `check` also read an older archive without that folder.

## 2. The interactive prompts in a real terminal

- **Item:** in PowerShell and in Git Bash, run `./compose.ps1 down -Volumes` (and
  `./compose.sh down --volumes`) and answer with a wrong word: it must print "confirmation
  did not match" and stop nothing. Run `./db.ps1 drop development` and answer `postit` once
  when the dev database may be emptied. Run `./vault.ps1 check` and `./vault.sh check` and
  confirm the `Vault password:` prompt appears and the typing is not echoed.
- **Why an agent cannot:** the tests feed answers through a pipe; how the prompt looks and
  reads on a real console needs a person at it.
- **Trigger:** the next time any of these commands is needed.

## 3. Restart the development stack after the password rotation

- **Item:** `./doctor.ps1` currently reports `FAIL identity`: `postit-zitadel` still uses the
  pre-rotation database password and cannot log in (`failed SASL auth` in its logs). Run
  `./compose.ps1 up` so compose recreates the containers with the new vault values, then
  `./doctor.ps1` again. Its `migration` row also warns that the development `postit`
  database has no migration applied; start the server once (`cargo dev` from `backend/`) so
  it applies `0001_init.sql`.
- **Why an agent cannot:** restarting the shared dev stack and the server is the
  maintainer's call while they work in it.
- **Trigger:** before the next development session.
- **Also:** the development server still holds two leftover scratch databases from earlier
  phases, `postit_prepare_c3` and `postit_prepare_d2`; drop them through `./db.ps1 psql
  development` when convenient.
