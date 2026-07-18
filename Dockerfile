# syntax=docker/dockerfile:1.7
FROM ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7 AS xray

FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY apps/ket-desktop/src-tauri ./apps/ket-desktop/src-tauri
COPY crates ./crates
RUN cargo build --locked --release --package ket-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 ket \
    && useradd --uid 10001 --gid ket --system --no-create-home ket \
    && install --directory --owner=ket --group=ket --mode=0700 /var/lib/ket /var/lib/ket-dataplane

COPY --from=builder /build/target/release/ket-server /usr/local/bin/ket-server
COPY --from=xray /usr/local/bin/xray /usr/local/bin/xray

USER ket
EXPOSE 8787
VOLUME ["/var/lib/ket", "/var/lib/ket-dataplane"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:8787/healthz"]

ENTRYPOINT ["/usr/local/bin/ket-server"]
