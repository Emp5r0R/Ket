# OpenVPN over stunnel TLS

Ket carries OpenVPN TCP inside a certificate-verified stunnel TLS connection. The server and Linux/Windows/Android adapters are implemented. Android uses Ket's certificate-pinned TLS socket carrier, which is wire-compatible with the server stunnel listener, and a private OpenVPN management/TUN-descriptor bridge.

## Security model

- stunnel 5.79 provides the outer TLS 1.2-or-newer connection, verifies the advertised hostname, and trusts only the CA material delivered in the authenticated session manifest.
- OpenVPN 2.7.5 independently verifies the inner server certificate, requires `remote-cert-tls server`, uses `tls-crypt`, disables compression, and permits only AEAD data ciphers.
- The client receives a lease-scoped 12-character username and a distinct 44-character data-plane password. Releasing, expiring, or revoking the lease makes authentication fail and the agent removes any matching connected client.
- The OpenVPN management socket is Unix-only inside the capability-limited agent container. Its bearer-authenticated reconciliation API and the control-plane auth callback exist only on the private `openvpn-control` network.
- The agent writes a root-only mode-`0600` runtime config under `/run` so OpenVPN can pass the internal auth URL and bearer to its restricted script environment. The bearer is absent from the image and process arguments.
- Client credentials, both CA documents, `tls-crypt` material, the stunnel configuration, and the password-protected OpenVPN management file are mode `0600` ephemeral files removed after shutdown.
- Android never writes the scoped username/password into the OpenVPN profile. It supplies them only over a mode-`0600` private Unix management socket, validates pushed interface/DNS settings, and keeps the established TUN under the foreground `VpnService` fail-closed guard.

This transport looks like generic TLS on the wire, but it is not HTTP and makes no Proton Stealth compatibility claim. Networks that actively fingerprint or require a real HTTP exchange may block it; Ket should then prefer XHTTP/TLS Stealth or WireGuard WebSocket/TLS.

## Server files

Create these paths before enabling the overlay:

```text
secrets/openvpn/
  ca.crt
  server.crt
  server.key
  stunnel-ca.crt
  tls-crypt.key
secrets/openvpn-stunnel/
  fullchain.pem
  privkey.pem
```

`ca.crt` signs the OpenVPN server certificate. `stunnel-ca.crt` is the trust chain for `fullchain.pem`; keep it at or below 8 KiB because the authenticated manifest carries it to the client. This bound supports current multi-certificate Let's Encrypt chains while rejecting unbounded material. `server.crt` must have server usage, and both private keys must remain readable only by the deployment operator. Generate the OpenVPN static key with the pinned image:

```bash
docker build -t ket-control-plane:local .
docker run --rm --user 0:0 \
  --entrypoint /usr/local/bin/openvpn \
  -v "$PWD/secrets/openvpn:/out" \
  ket-control-plane:local --genkey tls-crypt /out/tls-crypt.key
chmod 0700 secrets/openvpn secrets/openvpn-stunnel
chmod 0600 secrets/openvpn/server.key secrets/openvpn-stunnel/privkey.pem
chmod 0644 secrets/openvpn/ca.crt secrets/openvpn/server.crt \
  secrets/openvpn/stunnel-ca.crt secrets/openvpn/tls-crypt.key \
  secrets/openvpn-stunnel/fullchain.pem
```

The shared `tls-crypt` key is delivered only inside an authenticated manifest, but its host file must be readable by the rootless control-plane UID. Its containing directory remains mode `0700`, and Compose mounts only the three required files instead of exposing either server private key. Use an operator-controlled private CA or a normal public certificate chain for stunnel. The public DNS name in the certificate must equal `KET_OPENVPN_SNI`.

## Environment and start

Set the OpenVPN section in `.env`. Generate the manager and auth values independently; they must not equal the admin token or each other.

```bash
openssl rand -base64 48
openssl rand -base64 48

set -a; . ./.env; set +a
./packaging/validate-env.sh
docker compose -f compose.yaml -f compose.openvpn.yaml config --quiet
docker compose -f compose.yaml -f compose.openvpn.yaml up --build -d
docker compose -f compose.yaml -f compose.openvpn.yaml ps
```

The preflight checks required files, certificate/key envelopes, token independence, capacity, and the TCP bind collision with VLESS + REALITY. The Rust server repeats manifest and material bounds before listening.

## Ingress and Cloudflare

Open one stateful OCI and host-firewall TCP rule for `KET_OPENVPN_PUBLIC_PORT`. The DNS record must resolve directly to the server unless a compatible Layer 4 proxy is used. Ordinary Cloudflare orange-cloud HTTP proxying and Cloudflare Tunnel do not forward arbitrary stunnel TLS.

REALITY also defaults to raw TCP `443`. Two containers cannot bind the same host address and port. When both are enabled, assign OpenVPN another public IP or a different port such as `8443`; the preflight rejects an identical bind pair.

## Runtime checks

Before deployment, run the host-local nested TLS and lease lifecycle check:

```bash
./packaging/verify-openvpn-handshake.sh
```

It requires `openvpn`, `stunnel`, `openssl`, `curl`, `jq`, `nc`, `shuf`, and `socat`; it builds the two Ket server binaries when needed. The disposable test generates independent inner and outer CAs, enrolls through the real control API, connects a real OpenVPN client through certificate-pinned stunnel, verifies scoped authentication and nonzero management counters, then proves that session release and grant revocation disconnect the client. It uses OpenVPN `dev null` because it is designed to run without host network mutation. It therefore proves the nested handshake and control lifecycle, not TUN creation, routing, NAT, DNS, or Internet packet flow.

On the deployment host, follow with the container and manager checks:

```bash
docker compose -f compose.yaml -f compose.openvpn.yaml logs --tail=100 openvpn-agent openvpn-stunnel
docker compose -f compose.yaml -f compose.openvpn.yaml exec openvpn-agent \
  curl --fail --silent --show-error \
  -H "Authorization: Bearer $KET_OPENVPN_MANAGER_TOKEN" \
  http://127.0.0.1:8789/healthz
```

The manager health endpoint is intentionally not host-published. Verify TUN-backed end-to-end Internet traffic, DNS, counters, and revocation from a packaged Linux, Windows, or Android client before production use. Docker builds the OpenVPN/stunnel binaries natively, so the same image definition supports `linux/amd64` and `linux/arm64`.
