# What a Ket server knows

A Ket server keeps as little as it can about the people who use it. This page lists everything it holds, and what someone who controls the network path (a CDN, a tunnel, or the hosting provider) can see.

## Stored on the server

The state file (`/var/lib/ket/state.json` in the control-plane volume) holds, for each invite:

- an ID, the Argon2id hash of its access code (never the code itself), the label the operator chose, the device limit, the allowed protocols, whether it is paused, and when it was created and expires;
- if it has a data allowance: the allowance, when it began, and one running total of what its devices used this period, for all of them together. A new period replaces the total; earlier periods are not kept.

An invite is forgotten, label and all, 7 days after it expires, or at once when it is revoked.

For each connected device:

- a session ID, BLAKE2s hashes of its two tokens (never the tokens), the device name the app sent, when the session started and expires, and the slot that picks its port or tunnel address.

A session is deleted when the device disconnects or its lease runs out. The server stores no IP addresses, no traffic history, no visited sites, and no DNS queries. The byte counters an operator sees come live from the protocol engines and are not saved; only an allowance's single running total is. Fair sharing ([FAIRNESS.md](FAIRNESS.md#fair-sharing-of-the-connection)) runs in the kernel and keeps nothing.

The server's `.env` holds its own keys (TLS, node identity, protocol keys). Per-device protocol credentials are either derived from those keys and the session ID when they are needed, or kept only as the token hash above; none is stored in a form that could be handed to someone else.

## Kept in memory only

So an operator can see why a device cannot connect, `sudo ket invites show ID` lists what devices recently did with that invite: joined, checked in, disconnected, were refused and why (wrong code, paused, data allowance used up, device limit, app too old), and whether their traffic reached the OpenVPN or Hysteria2 server. Each entry names the app and the start of its session ID, with a count and the last time; no address. The server keeps the last 24 entries per invite in memory, never writes them to disk, forgets them when it restarts, and drops an invite's entries when the invite is revoked or forgotten.

Sharing with nearby devices ([SHARING.md](SHARING.md)) happens entirely in the app: the share's password, the devices using it, and how many wrong passwords were tried stay in the app's memory and end with the share. The server sees the shared traffic as the sharing device's own.

## Logs

- The HTTPS edge keeps no access log and logs errors only.
- Xray keeps no access or DNS log. Hysteria2, stunnel, and wstunnel log warnings only, so they do not record each connection's address.
- Every container's log is capped at three rotated 1 MB files, so the oldest lines disappear within hours or days. `sudo ket logs` shows what is there now.

Errors can still name a client address, for example when a connection breaks mid-handshake. Those lines age out with the rest.

## What the network path sees

Apps that join with an invite seal every control request (joining, renewing, disconnecting) to the server's key, through one route, `POST /v1/sealed` (see [INVITE_CAPSULE.md](INVITE_CAPSULE.md#sealed-control)). A CDN or tunnel that terminates TLS for your domain, such as Cloudflare, sees only that the app sent opaque bytes and got opaque bytes back: not the access code, not the session token, not the protocol credentials, not the app version, and not whether joining worked. Apps that joined with a server address and access code instead of an invite cannot seal and send these in the clear inside TLS.

The protocols themselves are encrypted end to end between the app and the server. A front that relays a protocol (XHTTP through Cloudflare, for example) can see that protocol's per-device ID and which sites a device connects to by name, in the same way it would for any website it proxies. Use REALITY, Hysteria2, Shadowsocks, WireGuard, or OpenVPN directly when that matters.

Your hosting provider can see that a device's address talks to your server, like any other traffic.
