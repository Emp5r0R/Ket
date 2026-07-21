# Shadowsocks 2022 deployment

Ket implements SIP022 Shadowsocks 2022 with the maintained shadowsocks-rust 1.24.0 `ssmanager` and `sslocal` executables. It fixes the cipher to `2022-blake3-aes-256-gcm` and enables TCP plus UDP. This is a native direct transport, not an HTTP/CDN protocol and not Proton Stealth.

## Server configuration

Generate a server-only derivation key independently of every other Ket secret:

```bash
openssl rand -base64 48
```

Set these values in `.env`:

```dotenv
KET_SHADOWSOCKS_ENABLED=true
KET_SHADOWSOCKS_PUBLIC_HOST=vpn.example.com
KET_SHADOWSOCKS_PORT_START=20000
KET_SHADOWSOCKS_PORT_END=20999
KET_SHADOWSOCKS_CREDENTIAL_KEY=<independent-generated-value>
```

The inclusive port range must contain at least `KET_MAX_SESSIONS` ports. Ket permits at most 1,500 sessions when Shadowsocks is enabled because `ssmanager` creates one TCP+UDP server per active lease. Each persisted lease has a crash-safe resource slot; Ket derives its public port and 32-byte standard-base64 SIP022 key from that slot and the server-only key. The plaintext client key is never persisted.

Validate and start the overlay:

```bash
set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose -f compose.yaml -f compose.shadowsocks.yaml config --quiet
docker compose -f compose.yaml -f compose.shadowsocks.yaml up --build -d
docker compose -f compose.yaml -f compose.shadowsocks.yaml ps
```

The image build downloads the official `ssmanager` release for the Docker target architecture, verifies its pinned SHA-256 digest, and supports native `linux/amd64` and `linux/arm64` hosts. Docker images are architecture-specific at runtime: an ARM host normally runs the ARM64 image, while an AMD64 host runs the AMD64 image. Cross-architecture emulation is possible but slower and is not required by Ket.

## DNS and ingress

`KET_SHADOWSOCKS_PUBLIC_HOST` must resolve directly to the server. With Cloudflare DNS, use a DNS-only record (grey cloud). Cloudflare Tunnel and ordinary orange-cloud HTTP proxying do not forward native Shadowsocks TCP/UDP traffic.

Open the entire inclusive port range for both protocols in every firewall layer:

- OCI VCN security list or network security group: source `0.0.0.0/0`, IP protocol TCP, destination range `20000-20999`.
- OCI VCN security list or network security group: source `0.0.0.0/0`, IP protocol UDP, destination range `20000-20999`.
- Host firewall: the same TCP and UDP range.

Use the actual configured range rather than the example. Do not open UDP `6100`: the unauthenticated manager API is isolated on the dedicated `shadowsocks-control` Compose network and has no published host port. The container runs as UID `10001`, has a read-only root filesystem, no Linux capabilities, and an ACL that rejects private, loopback, link-local, multicast, and other special-purpose IP ranges.

## Lease lifecycle

The control plane does not report ready until it has reconciled every persisted active lease with `ssmanager`. Provisioning a new session removes any stale server at that lease port, adds the exact expected key/method/mode, and verifies the manager state before returning the manifest. Release, expiry, or grant revocation removes the port and verifies that it no longer exists.

The upstream manager reports one combined byte counter per port rather than trustworthy sent/received directions. Ket therefore leaves directional data-plane traffic unavailable instead of mislabeling the aggregate value. Android and desktop still collect local directional counters for their UI.

## Client support

Linux and Windows packages bundle a checksum-pinned `sslocal`, validate the exact Ket profile, write a mode-`0600` ephemeral configuration, establish a TCP+UDP loopback SOCKS endpoint, and feed the existing supervised full-route bridge. The configuration is deleted after readiness.

Android bundles the official 64-bit `sslocal` payload as `libsslocal.so` for `arm64-v8a` and `x86_64`. The upstream binaries target Android API 28, so Ket fails closed on API 26-27 and on 32-bit devices, allowing the ranked selector to fall back to Hysteria2. Before installing the VPN route, Android pins and excludes the resolved server address and verifies a certificate-authenticated TLS request through the local Shadowsocks SOCKS endpoint.

The disposable local traffic harness downloads checksum-pinned `ssmanager` and `sslocal` binaries, starts the real Ket control plane, rejects a wrong key, carries HTTP over the issued SOCKS endpoint, observes the manager byte counter, and proves both session release and grant revocation stop traffic:

```bash
./packaging/verify-shadowsocks-traffic.sh
```

The harness deliberately omits the production egress ACL so its loopback HTTP origin is reachable. It proves the shipped Shadowsocks 2022 TCP engine and lease lifecycle, not UDP forwarding, public ingress, ACL behavior, or restricted-network Android behavior. Those remain deployment release gates.
