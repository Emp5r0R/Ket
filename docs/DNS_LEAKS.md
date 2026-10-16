# DNS leaks

While Ket is connected, every name lookup and all IPv6 traffic must go through the tunnel. Routing everything into the tunnel is not enough on its own:

- **Windows** asks every network adapter's resolvers in parallel (Smart Multi-Homed Name Resolution), and it reaches link-local IPv6 resolvers that no route can steer.
- **Linux with systemd-resolved** (Ubuntu, Fedora, and most desktops) sends each network's lookups out of that network. When the network changes its DNS servers, for example on a DHCP renewal, resolved also rewrites the resolver file that Ket pointed at the tunnel.

## What Ket does

For as long as a desktop tunnel is up, Ket keeps the system's lookups inside it. The code is in `crates/ket-client-core/src/leak_guard/` (Windows) and `crates/ket-client-core/src/system_dns/` (Linux).

| Platform | What keeps lookups inside |
| --- | --- |
| Windows | Before the bridge starts, Windows Filtering Platform filters block DNS, DNS over TLS, LLMNR, and NetBIOS name lookups on every adapter. They also block IPv6 beyond the local link, because the bridge routes only IPv4 on Windows. Once the `Ket` adapter exists, it is allowed to carry everything, and the engines may still reach the Ket server directly. The filters live in a dynamic session, so Windows removes them if Ket exits or crashes. |
| OpenVPN on Windows | OpenVPN's own `block-outside-dns`. |
| Linux with systemd-resolved | The tunnel link (`ket0`, or `ketvpn0` for OpenVPN) becomes resolved's resolver for every domain (`~.`). Lookups through the resolved API or its `127.0.0.53` stub therefore go into the tunnel, even after the network changes its servers. Ket leaves resolved's `/etc/resolv.conf` alone. |
| Linux without systemd-resolved | `/etc/resolv.conf` points into the tunnel while connected and is restored afterwards. A crash is repaired when the tunnel service next starts. |
| Android | `VpnService` sends DNS for every app that uses Ket into the tunnel. Apps that split tunneling leaves out use the network directly, by design. |

If this cannot be set up, the connection fails with "failed to keep DNS inside the protected tunnel" instead of connecting without it.

Up to v1.1.0, Ket rewrote `/etc/resolv.conf` on every Linux computer. On systemd-resolved desktops that file belongs to the `systemd-resolve` user. The tunnel service's sandbox gives root only `CAP_NET_ADMIN` and `CAP_SYS_ADMIN`, so the write was refused and every desktop protocol failed to connect, with "failed to route system DNS through the protected tunnel". The lab reproduces this by running the client under the shipped unit's sandbox.

## Checking for leaks

Each check looks up unique names and reports any that leave outside the tunnel.

**Linux, any computer** (as root, while connected):

```bash
sudo scripts/dns-leak-check.sh
```

It captures every interface except the tunnel. It then looks up names through glibc, the systemd-resolved API, and `dig`, and opens an IPv6 connection. It reports any probe name, any other DNS, and any non-local IPv6 that left outside the tunnel. Run it with `--baseline` and without Ket to see that it catches direct lookups on your network.

**Linux lab** (Docker, no changes to this computer):

```bash
scripts/dns-leak-lab.sh ubuntu@your-server --ssh-key KEY \
  --protocol vless_xtls_reality --protocol stealth --protocol shadowsocks2022 \
  --protocol xor_scrambled --protocol wire_guard
```

The lab connects a disposable Ubuntu 24.04 container to your server, using an invite it creates and later revokes, with the same client code the desktop runs. It tests two setups:

- systemd-resolved with a link-local IPv6 resolver, including a mid-session DNS change;
- a plain `/etc/resolv.conf`.

For each setup it runs the check above and confirms that disconnecting restores the resolver configuration. `--client FILE` tests another build, such as an older commit's, to compare.

**Windows** (PowerShell, while connected):

```powershell
powershell -ExecutionPolicy Bypass -File scripts\dns-leak-check.ps1
```

It confirms three things:

- the system resolver answers through the tunnel;
- every other adapter's resolver, IPv4 and IPv6 including link-local, cannot be reached from that adapter's own address, which is the path Windows' parallel lookups take;
- IPv6 does not reach the internet outside the tunnel.

Windows CI runs the real bridge both with and without the guard. Without it, this check must find the leak; with it, the check must find none. CI also tests the filters directly.

**Android emulator** (Wi-Fi off, Ket connected):

```bash
scripts/android-dns-leak-check.sh
```

It captures the emulator's cellular network card from the computer, so it sees exactly what leaves the device. It then looks up names and pings IPv6 from inside the device. The `adb emu network capture` console cannot see the emulator's Wi-Fi card, which is why Wi-Fi must be off. Run it with `--baseline` before connecting.

## Results

All of these were run on 2026-10-16 against the canary.

- **Linux lab, before the guard** (client from `5b2ed1e`):
  - While connected, systemd-resolved sent lookups to the network's resolver.
  - After the network changed its DNS servers, every lookup path leaked, including glibc and `dig`.
  - The plain setup did not leak.
- **Linux lab, with the fixes,** running the client under the shipped tunnel service's sandbox:
  - VLESS + REALITY, XHTTP Stealth, Shadowsocks 2022, Hysteria XOR, and WireGuard TLS connected in both setups.
  - Each loaded a site by name through the server.
  - None leaked, including after the DNS change.
  - Disconnecting restored the resolver configuration.
- **OpenVPN TLS** did not connect in the lab ("OpenVPN startup timed out"). It fails the same way with the client from before these changes, so the cause predates them; it is tracked separately. The OpenVPN device name is checked on its own: OpenVPN opens `ketvpn0`.
- **WireGuard TLS:** the lab also found that sites by name did not load through it, on desktop or Android. Its Xray resolved names through the system resolver, which during the session is the tunnel's own virtual DNS. Xray now resolves through the tunnel instead; see the WireGuard note below.
- **Android emulator** (API 34, debug build, every app using Ket):
  - The baseline caught both probe lookups and the IPv6 ping.
  - While connected over VLESS + REALITY, no lookup, DNS, or IPv6 traffic left the device.

The Windows checks run in Windows CI; they have not been run on a physical Windows computer yet.

## WireGuard name resolution

Up to v1.1.0, WireGuard TLS connected, but nothing could be reached by name. The bridge (or Android's mapped DNS) hands Xray a hostname. Xray's WireGuard outbound then resolved it with the system resolver, which answered with the bridge's own virtual address, and the server could not route that. Xray now asks 1.1.1.1 and 1.0.0.1 itself, through the WireGuard tunnel, for IPv4 only, since the WireGuard peer carries only IPv4. OpenVPN already used these resolvers through its tunnel.

The Linux lab confirmed the fix on desktop: sites load by name through WireGuard TLS, and nothing leaks. Android's WireGuard TLS runs only on arm64 phones, so its fix is covered by unit tests and by the pinned Xray accepting the configuration; it still needs a check on a physical phone.
