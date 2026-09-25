# Builds the postit binary (postit-server crate). Not yet wired into compose.dev.yaml's
# nginx-app placeholders; used starting plan 02 phases P6/P7.

FROM rust:1.98.1-slim AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --package postit-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/postit /usr/local/bin/postit
ENTRYPOINT ["/usr/local/bin/postit"]
