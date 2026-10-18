# Sharing with nearby devices

Ket can share its connection with devices that cannot run Ket themselves, such as a game console, a work laptop, or a friend's phone joined to your hotspot. Sharing is off until you start it.

## Using it

1. Connect Ket.
2. Open **Settings → Share with nearby devices**, choose the network (your hotspot, or the Wi-Fi network you are on), and tap **Start sharing**.
3. On the other device, open its Wi-Fi settings, set the proxy to **Manual**, and enter the address and port Ket shows. Sign in with the user name and password when the browser asks. Apps that support proxies can scan the QR code instead; it holds `http://ket:PASSWORD@ADDRESS:PORT`.

The share speaks both HTTP proxy and SOCKS5 on the same port. While it runs, a banner on the main screen shows the address and how many devices are connected.

Sharing stops when you tap **Stop**, when you disconnect Ket, or when your computer or phone leaves the network it shares on. Each start draws a new password, so a password you gave out last time stops working.

Use it on networks you trust, such as your own hotspot. The link between the other device and yours is not encrypted by Ket, so others on that network could read the password and plain `http://` pages; HTTPS sites stay encrypted as always. If Ket reports refused sign-ins you did not make, stop and start sharing for a new password.

On a computer with a firewall, allow Ket to accept connections on private networks (Windows asks the first time). On Linux with `ufw` enabled, allow the port Ket shows, for example `sudo ufw allow from 192.168.1.0/24 to any port 39740 proto tcp`.

## How it works

The share is one proxy, `crates/ket-share`, used by the desktop app directly and by Android through `crates/ket-share-android` (`libket_share_jni.so`). It runs in the app, never in the privileged tunnel service, so nothing a nearby device sends is read by a privileged process.

- **Where it listens.** Only on one private (RFC 1918) address of the network you chose, never on every address and never on mobile data, whose carrier addresses are often private too. It tries port 39740 first, so a device set up once keeps working, and takes a free port when that is busy. The port does not change while Ket reconnects or switches protocol: the share sits in front of the tunnel, not inside an engine, so it works the same with every protocol.
- **Signing in.** Every connection must sign in: SOCKS5 user name and password (RFC 1929) or HTTP `Proxy-Authorization: Basic`. The password is 12 characters drawn by the operating system's generator from 31 letters and digits that are hard to confuse (59 bits). A device that gets it wrong five times within a minute is turned away for a minute, and past 30 wrong tries in a minute from all addresses together, only devices already signed in can open new connections. Passwords are compared in constant time and kept in memory only.
- **Only through the tunnel.** Every connection and every name lookup for a nearby device is tied to the tunnel itself: on Linux with `SO_BINDTODEVICE` on Ket's interface, on Windows with `IP_UNICAST_IF`, on Android with the VPN network (`android_setsocknetwork`, `android_getaddrinfofornetwork`). Desktop lookups go to the tunnel's own resolver on a socket bound the same way, never to the system resolver. With no tunnel, while Ket reconnects or after it disconnects, connections fail and the device is told Ket is not connected (HTTP 503); nothing leaves through the local network.
- **Only the Internet.** Destinations are checked after any name lookup, so a name that resolves to a private address is refused too. Loopback (the tunnel service's control port included), private networks, link-local, carrier-grade NAT, documentation, multicast, and other reserved ranges are refused, as are names such as `localhost`, `router`, or `printer.local`. The only exception is the stand-in addresses the tunnel's own resolver hands out for names (198.18.0.0/15 on desktop, 240.0.0.0/4 on Android), and only when they come from that resolver.
- **What it carries.** TCP: `CONNECT` and SOCKS5 `CONNECT` for anything, plus plain `http://` requests, which are forwarded one per connection without the password and with `Connection: close`. SOCKS5 UDP is not offered, so apps fall back to TCP.
- **Limits.** At most 256 connections at once; 15 seconds to sign in and name a destination; 20 seconds to reach it.

## Testing

`crates/ket-share` tests the SOCKS5 and HTTP handshakes, cut-short and malformed input, the sign-in limit, the destination rules, the tunnel-bound DNS client, and the server end to end with a stand-in tunnel. The desktop and Android apps test their network choices, the share's lifecycle, and their screens.

`scripts/share-lab.sh <ssh-target> [--ssh-key FILE] [--protocol NAME]...` runs the desktop share unprivileged in a Docker container and uses it from a second container, while each protocol connects and disconnects underneath it. It checks that over HTTP and SOCKS5 a site sees the tunnel's exit; that plain pages load; that the computer itself cannot be reached through the share; that a wrong password is refused and guessing is stopped; that the port stays the same across protocols; that with no tunnel the device gets 503; and that the share's user sent nothing out of the computer's interface but its answers to the device, with no name lookup leaving it (counted by netfilter's owner match and checked against a positive control). It ends with `The share carried a nearby device through every protocol, and only through the tunnel.`
