# The postit-server image: the `postit` binary (postit-server crate). Built by the
# postit-api / postit-worker services in docker-compose.qa.yml and
# docker-compose.production.yml, and promoted unchanged from qa to production.
#
# Build context is the REPOSITORY ROOT (see .dockerignore there), because plan 02 P7 adds
# a Flutter stage that needs app/ beside server/:
#
#   docker build -f docker/server.Dockerfile -t postit-server:local .
#
# Plan 02 P1 target still to come: cargo-chef dependency layers, SQLX_OFFLINE=true, the
# Flutter web stage (P7), and a HEALTHCHECK on `postit healthcheck` (P6).

FROM rust:1.98.1-slim-trixie AS builder
WORKDIR /src
COPY server/ ./
RUN cargo build --profile dist --package postit-server

# Same Debian release as the builder, so the binary never links against a newer glibc
# than the runtime has.
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home postit
COPY --from=builder /src/target/dist/postit /usr/local/bin/postit
# The non-secret settings layers, at the same relative path `cargo run` sees from server/.
# local.toml and the vault never reach the build context (.dockerignore); secrets arrive at
# run time as POSTIT__… env vars from the vault's postit.env (compose `env_file:`).
WORKDIR /app
COPY server/config/default.toml server/config/qa.toml server/config/production.toml config/
USER postit
ENTRYPOINT ["/usr/local/bin/postit"]
