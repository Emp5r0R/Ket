# Shared client core

`ket-client-core` is the orchestration boundary shared by the Linux and Windows desktop shells. Android consumes the same contracts while providing `VpnService`-owned transport adapters instead of launching desktop route-management processes.

## Implemented lifecycle

1. `ControlEndpoint` accepts HTTPS and loopback HTTP by default. Embedded credentials, URL query secrets, fragments, redirects, ambient system proxies, oversized responses, and TLS older than 1.2 are rejected.
2. `KetClient::enroll` validates the 32-character access code locally, exchanges it once, validates the returned token/profile shapes, and retains the session only in memory.
3. `TransportSelector` ranks adapters using configured priority, optional protocol preference, recent latency, consecutive failures, and bounded cooldown.
4. `KetClient::connect` probes and attempts at most the configured number of transports. Every failure is recorded before fallback.
5. A serializable `ClientSnapshot` is published through a Tokio watch channel for UI parity. It contains no control token, transport credential, or secret option values.
6. `refresh`, `renew`, and the optional maintenance task update node health, capacity, traffic, expiry, and reconnect state. Authorization loss stops the tunnel and clears the in-memory session.
7. `disconnect` stops the local tunnel before releasing the server lease. A failed release is safe because the bounded server lease still expires.

## Desktop service adapters

The privileged service delegates protocol logic to maintained upstream engines. The desktop process uses `BrokerTransportAdapter`, which authenticates to that service over loopback and never launches engines directly.

Hysteria2 uses its official [cross-platform TUN mode](https://v2.hysteria.network/docs/advanced/Full-Client-Config/#tun). VLESS + REALITY uses Xray-core with a loopback SOCKS inbound and `tun2proxy` for full-route TUN ownership. Ket does not reimplement QUIC, REALITY, or their cryptography.

- TLS verification is mandatory and uses the server-provided SNI.
- Each adapter resolves and excludes every current server IP before adding full IPv4/IPv6 routes, preventing a tunnel routing loop.
- Only known transport options are accepted. Unknown fields, missing credentials, invalid Gecko bounds, invalid REALITY keys, unsupported flows, and downgrade-shaped fields fail closed.
- BBR defaults are preserved; the client does not invent unsafe bandwidth values.
- Configuration files are created in a private directory with mode `0600` on Unix, zeroed in the writer buffer, and removed after both connection and TUN readiness are observed.
- Raw engine output is discarded after extracting an allowlisted diagnostic category, preventing share URIs or credentials from entering app logs.
- The child process is supervised and killed on explicit disconnect, terminal failure, service shutdown, or desktop heartbeat expiry.

The desktop package bundles verified Hysteria, Xray, and `tun2proxy` binaries with the service installer. The GUI deliberately does not self-elevate. Hysteria TUN transports TCP and UDP but not ICMP, an upstream limitation that the UI diagnostics must state accurately.

## Integration sketch

```rust,no_run
use std::{sync::Arc, time::Duration};

use ket_client_core::{
    BrokerConfig, BrokerTransportAdapter, ControlEndpoint, HttpControlPlane, KetClient,
    SelectionPolicy,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let client = KetClient::new(
    ControlEndpoint::parse("https://ket.example.com")?,
    "Aadhish's workstation",
    Arc::new(HttpControlPlane::new()?),
    vec![Arc::new(BrokerTransportAdapter::new(BrokerConfig::from_env()?))],
    SelectionPolicy::default(),
)?;

let mut snapshots = client.subscribe();
client.enroll("A2345678901234567890123456789012").await?;
client.connect().await?;
let maintenance = client.spawn_maintenance(Duration::from_secs(15));

snapshots.changed().await?;
let ui_state = snapshots.borrow().clone();

client.disconnect().await?;
maintenance.shutdown().await;
# let _ = ui_state;
# Ok(())
# }
```
