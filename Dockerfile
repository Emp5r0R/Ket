# syntax=docker/dockerfile:1.7
FROM ghcr.io/xtls/xray-core:26.3.27@sha256:592ec4d11f656db95598d01e76dbcc6e002d67360b96a5436500a938230f52c7 AS xray

FROM debian:bookworm-slim AS shadowsocks
ARG TARGETARCH
ARG SHADOWSOCKS_VERSION=1.24.0
RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates curl xz-utils \
    && rm -rf /var/lib/apt/lists/* \
    && case "$TARGETARCH" in \
         amd64) target=x86_64-unknown-linux-gnu; sha256=5f528efb4e51e732352f5c69538dcc76e8cf8f6d1a240dfb5b748a67f0b05f65 ;; \
         arm64) target=aarch64-unknown-linux-gnu; sha256=dc56150cb263e1e150af33cc4c6542035aab3edf602e340842cca4138a4d5c51 ;; \
         *) printf 'unsupported Docker architecture: %s\n' "$TARGETARCH" >&2; exit 1 ;; \
       esac \
    && archive="shadowsocks-v${SHADOWSOCKS_VERSION}.${target}.tar.xz" \
    && curl --fail --location --proto '=https' --tlsv1.2 \
         --output "/tmp/${archive}" \
         "https://github.com/shadowsocks/shadowsocks-rust/releases/download/v${SHADOWSOCKS_VERSION}/${archive}" \
    && printf '%s  %s\n' "$sha256" "/tmp/${archive}" | sha256sum --check --strict - \
    && tar --extract --xz --file "/tmp/${archive}" --directory /tmp ssmanager \
    && install --mode=0755 /tmp/ssmanager /usr/local/bin/ssmanager

FROM debian:bookworm-slim AS wstunnel
ARG TARGETARCH
ARG WSTUNNEL_VERSION=10.6.2
RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && case "$TARGETARCH" in \
         amd64) target=linux_amd64; sha256=db6064cca0515b67f8652e201cff8e27553b8cbb7216b2e19241311e34868e6e ;; \
         arm64) target=linux_arm64; sha256=26bb36b856948255bec7cd71a39df5f8912acdd7a47a9ccd4044a9b80ced108d ;; \
         *) printf 'unsupported Docker architecture: %s\n' "$TARGETARCH" >&2; exit 1 ;; \
       esac \
    && archive="wstunnel_${WSTUNNEL_VERSION}_${target}.tar.gz" \
    && curl --fail --location --proto '=https' --tlsv1.2 \
         --output "/tmp/${archive}" \
         "https://github.com/erebe/wstunnel/releases/download/v${WSTUNNEL_VERSION}/${archive}" \
    && printf '%s  %s\n' "$sha256" "/tmp/${archive}" | sha256sum --check --strict - \
    && tar --extract --gzip --file "/tmp/${archive}" --directory /tmp wstunnel \
    && install --mode=0755 /tmp/wstunnel /usr/local/bin/wstunnel

FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY apps/ket-desktop/src-tauri ./apps/ket-desktop/src-tauri
COPY crates ./crates
COPY vendor ./vendor
RUN cargo build --locked --release --package ket-server --bins

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates curl iproute2 iptables wireguard-tools \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 ket \
    && useradd --uid 10001 --gid ket --system --no-create-home ket \
    && install --directory --owner=ket --group=ket --mode=0700 /var/lib/ket /var/lib/ket-dataplane

COPY --from=builder /build/target/release/ket-server /usr/local/bin/ket-server
COPY --from=builder /build/target/release/ket-wireguard-agent /usr/local/bin/ket-wireguard-agent
COPY --from=xray /usr/local/bin/xray /usr/local/bin/xray
COPY --from=shadowsocks /usr/local/bin/ssmanager /usr/local/bin/ssmanager
COPY --from=wstunnel /usr/local/bin/wstunnel /usr/local/bin/wstunnel
COPY packaging/shadowsocks-server.acl /etc/shadowsocks/ket-server.acl
COPY THIRD_PARTY_NOTICES.md /usr/share/doc/ket/THIRD_PARTY_NOTICES.md

USER ket
EXPOSE 8787
VOLUME ["/var/lib/ket", "/var/lib/ket-dataplane"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD ["curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:8787/healthz"]

ENTRYPOINT ["/usr/local/bin/ket-server"]
