# The postit-server image: the `postit` binary (postit-server crate). Built by the
# postit-api / postit-worker services in docker-compose.qa.yml and
# docker-compose.production.yml, and promoted unchanged from qa to production.
#
# Build context is the REPOSITORY ROOT (see .dockerignore there), because plan 02 P7 adds
# a Flutter stage that needs app/ beside server/:
#
#   docker build -f docker/server.Dockerfile -t postit-server:local .
#
# Dependencies are cooked in their own cargo-chef layer, the build is offline for sqlx
# (SQLX_OFFLINE=true), and the runtime stage carries a HEALTHCHECK on `postit healthcheck`.
# Still to come: the Flutter web stage (plan 02 P7).

FROM rust:1-slim-trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY server/ ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
# rust-toolchain.toml sits beside the recipe so the cooked dependencies use the same toolchain.
COPY server/rust-toolchain.toml ./
# `-p postit-server` matches the build below, so feature unification is identical and the
# cooked layer is reused instead of partly rebuilt (the recipe also covers xtask).
RUN cargo chef cook --profile dist --package postit-server --recipe-path recipe.json
COPY server/ ./
ENV SQLX_OFFLINE=true
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
EXPOSE 8080 8081 8082
HEALTHCHECK --interval=15s --timeout=6s --start-period=30s --retries=3 CMD ["postit", "healthcheck"]
USER postit
ENTRYPOINT ["/usr/local/bin/postit"]
