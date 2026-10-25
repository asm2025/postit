# B5 agent kit and mock agent: deferred issues

## Media requests carry IDs, but opening media needs a full asset

`PublishRequest` carries only media asset IDs, but `MediaStore::open` needs a full `MediaAsset`.
The conformance `validate` check and the test `EchoAgent` therefore pass an empty media list or a
stub asset built from the ID, so media-rule validation is not exercised against real assets.

Why it cannot be fixed now: it needs the media crate and the asset model (B4) and the engine's
request assembly (D1), neither of which exists in B5.

Trigger: the B4 media phase and the D1 publishing engine. Together they must decide how a request
resolves IDs to assets, and then extend the conformance fixture with real assets.
