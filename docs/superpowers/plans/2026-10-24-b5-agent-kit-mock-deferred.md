# B5 agent kit and mock agent: deferred issues

## Media requests carry IDs, but opening media needs a full asset

`PublishRequest` carries only media asset IDs, but `MediaStore::open` needs a full `MediaAsset`.
The conformance `validate` check and the test `EchoAgent` therefore pass an empty media list or a
stub asset built from the ID, so media-rule validation is not exercised against real assets.

Why it cannot be fixed now: it needs the media crate and the asset model (B4) and the engine's
request assembly (D1), neither of which exists in B5.

Trigger: the B4 media phase and the D1 publishing engine. Together they must decide how a request
resolves IDs to assets, and then extend the conformance fixture with real assets.

## `cargo deny check` fails on new RustSec advisories in unchanged dependencies

`cargo deny check advisories` reports 13 advisories: RUSTSEC-2026-0346, 0347, 0348, 0349 (`rustls`),
0367, 0368, 0369 (`zip`), 0374 (`basic-toml`, unmaintained), 0381 (`webpki-roots`),
0382 (`rustls-webpki`), 0385 (`ctutils`), 0386 (`tokio`), and 0387 (`crypto-bigint`). B5's only
`Cargo.lock` change against `stage` is the two new workspace crates (`postit-agent-kit`,
`postit-agent-mock`); none of the flagged crates changed on this branch.

Why it cannot be fixed now: the fix is a workspace-wide dependency upgrade (a `zip` major bump,
TLS stack and tokio updates, and `basic-toml` has no safe upgrade) that needs its own review and a
full retest, and is unrelated to the agent kit. `server/deny.toml` is not changed to ignore them.

Impact: the `server (linux)` CI job runs `cargo deny check` on every pull request and on pushes to
`main` and `stage`, with no advisory ignores beyond RUSTSEC-2023-0071. It will fail on these
advisories, so the B5 pull request cannot go green until they are resolved.

Trigger: before the B5 pull request merges, run a dependency refresh (`cargo update` for the
patched `rustls`, `rustls-webpki`, `webpki-roots`, `tokio`, `crypto-bigint`, `ctutils`, then `zip`
and `basic-toml` handled individually) as its own commit or branch, targeting `stage`.
