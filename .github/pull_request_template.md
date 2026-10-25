## Summary

<!-- What changes and why. -->

## Checks

- [ ] Quality gates pass (`cargo fmt --check`, clippy `-D warnings`, `cargo test --workspace`; web: typecheck, lint, format, test).
- [ ] `cargo xtask openapi` re-run if any route or schema changed; `/api/v1` changes are additive only.

## Migration review (only if `backend/crates/data/migrations/` changed)

During an upgrade the new `api` container migrates while the old `worker` still runs, so a migration must work with the previous release's code (docs/deployment.md, "Upgrades").

- [ ] Only adds: new tables, new nullable or defaulted columns, new indexes (`CONCURRENTLY` where the table can be large).
- [ ] Drops nothing and renames nothing that the previous release reads. A removal waits for a later release (expand, then contract).
- [ ] No new `NOT NULL` column without a default on an existing table.
- [ ] Needs no superuser right beyond the database owner role (trusted extensions only, such as `citext`).
- [ ] The `.sqlx` caches are regenerated.
