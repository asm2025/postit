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

The advisory database now flags `rustls`, `rustls-webpki`, `webpki-roots`, `tokio`, `crypto-bigint`,
`ctutils`, `zip`, and the unmaintained `basic-toml` (RUSTSEC-2026-0346 to 0349, 0367 to 0369, 0374,
0381, 0382, 0385 to 0387). B5's only `Cargo.lock` change is the two new workspace crates
(`postit-agent-kit`, `postit-agent-mock`); none of the flagged crates changed on this branch.

Why it cannot be fixed now: the fix is a workspace-wide dependency upgrade (a `zip` major bump,
TLS stack and tokio updates, and an unmaintained crate with no safe upgrade) that needs its own
review and a full retest, and is unrelated to the agent kit.

Trigger: a dependency-refresh task before the first production deployment (plan 03 G1), or earlier
if the CI `cargo deny` job blocks a merge.
