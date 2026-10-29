# B5 agent kit and mock agent: deferred issues

## Media requests carry IDs, but opening media needs a full asset

`PublishRequest` carries only media asset IDs, but `MediaStore::open` needs a full `MediaAsset`.
The conformance `validate` check and the test `EchoAgent` therefore pass an empty media list or a
stub asset built from the ID, so media-rule validation is not exercised against real assets.

Why it cannot be fixed now: it needs the media crate and the asset model (B4) and the engine's
request assembly (D1), neither of which exists in B5.

B4 status (2026-10-26): B4's half is done. `MediaService::assets(scope, ids)` resolves IDs to full
`MediaAsset`s, and `FsMediaStore::open` finds the stored file by `(owner_id, asset id)`, so the core
type needs no storage field. The request assembly in the engine and the conformance fixture with
real assets remain D1's.

Trigger: the D1 publishing engine. Together they must decide how a request
resolves IDs to assets, and then extend the conformance fixture with real assets.

Resolved (D1 task 6): `postit_publishing::request::assemble` resolves a variant's media IDs to full assets through `MediaService::assets`, and `ConformanceFixture.assets` hands real assets to the `validate` check; the mock's conformance test builds them through `FsMediaStore`.
