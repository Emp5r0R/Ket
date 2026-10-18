# Sharing a server fairly

Two optional controls keep one person from using up a server that others share. Both are off until you turn them on.

## Data allowances

An invite can carry a data allowance: how much its devices may use together each day, week, or month, uploads and downloads counted.

```sh
sudo ket invite --data 50GB/month                      # a new invite
sudo ket invites edit ID --data 10GB/week              # an existing one
sudo ket invites edit ID --data none                   # no limit again
sudo ket invites edit ID --reset-data                  # start this period's count again
```

`sudo ket invite` also asks for it, and `sudo ket invites` and `sudo ket invites show ID` show what each invite has used. Sizes are binary (1 GB is 1024 MB). Periods follow UTC: a day starts at midnight, a week on Monday, a month on the 1st.

When an invite's devices use up its allowance, Ket disconnects them and refuses them, with the time it works again, until the period starts over. You can also raise the allowance or reset the count. The apps show the allowance and what is used under **Metrics**. The desktop app connects again by itself once the period starts over; the Android app stops and says when it can connect again.

How it counts:

- Every 15 seconds the server reads each protocol's own byte counters for the devices of invites that have an allowance; invites without one are never read. A device can therefore go up to 15 seconds' worth of traffic past its allowance.
- Only one running total per invite is kept, for the current period; earlier periods are not kept ([PRIVACY.md](PRIVACY.md#stored-on-the-server)). The total is written to disk at least once a minute, so a server restart can miss at most about a minute of counting. Nothing is ever counted twice.
- Traffic from before an allowance was set does not count. Changing the period starts the count again; changing the amount keeps it.

## Fair sharing of the connection

When the server's connection is full, a device that opens many connections (a download manager, a torrent, a game update) can crowd out everyone else. Fair sharing stops that:

```sh
sudo ket shaping set 950mbit    # a little below what your provider sells
sudo ket shaping                # is it on?
sudo ket shaping off
```

It puts CAKE, the Linux queue manager, on the interface of the server's default route, at the rate you give, and shares that rate equally between the addresses it sends to. The rate must be a little below your plan's real speed (for example 950mbit on a 1 Gbit/s plan): above it, the queue forms at your provider instead and CAKE has nothing to share. Ket turns it on again after a reboot, and `ket doctor` warns if it stopped.

What it cannot do:

- It shares what the server sends to devices, not what devices upload.
- Devices that reach the server through Cloudflare arrive over a few shared connections, and devices behind one public address (a home router, a campus, mobile carrier NAT) look like one device, so it cannot tell those apart.

## Testing

The server's tests cover the calendar periods, how counters turn into a total (engines that restart, OpenVPN connecting again, a reading that failed, traffic before the allowance), the admin API, refusal and reset, and storing the total. The engine tests in `scripts/test-all.sh integration` run the real Shadowsocks and Xray servers, push more than 1 MB through a 1 MB allowance, and check that the device is cut off, refused with `423 grant_allowance_used`, and let in again after a reset; the OpenVPN test checks that its connection counters reach the invite's total.

`scripts/shaping-lab.sh` runs the real `ket shaping` against a real kernel. A server sends to two devices through a router that stands in for a 60 Mbit/s provider. One device pulls with eight connections and the other with one. Without fair sharing the first takes almost everything (about 52 against 3 Mbit/s). After `ket shaping set 50mbit` they get about 23 Mbit/s each. The lab ends with `Fair sharing gave a one-connection device ... next to an eight-connection one, and turned off cleanly.`

`scripts/allowance-lab.sh SSH_TARGET [--ssh-key FILE] [--protocol NAME]...` checks a real server. For each protocol it creates a one-device invite with 1 MB a day, connects with the shipped client code in a desktop-like container, downloads until `ket invites show` says the allowance is used up, checks the server disconnected the device and logged why, that the tunnel stopped carrying traffic, that a new device is refused with `grant_allowance_used`, and that `--reset-data` lets it in again; then it revokes the invite. Against the canary on 2026-10-18 it passed for VLESS + REALITY, HTTPS Stealth, Shadowsocks 2022, Hysteria XOR, and WireGuard TLS (cut off between 1.1 and 4.7 MB). OpenVPN TLS does not connect from the test network at all, so its allowance was not checked on a real server.
