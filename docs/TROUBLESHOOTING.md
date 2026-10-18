# When someone cannot connect

Three things help you find out why, without learning anything private about the person.

## 1. Ask for their diagnostic report

- Desktop: **Troubleshoot**, then **Copy logs**.
- Android: **Settings**, **Connection diagnostics**, then **Copy logs**.

The report contains no addresses, access codes, tokens, or sites they visited: the apps remove addresses, host names, links, invites, and anything that looks like a key before showing it. It says:

- the app version, the system (`Ubuntu 24.04`, `Android 14`), and the kind of network: `wifi`, `ethernet`, `cellular`, `other`, or `none`;
- your server's version and the start of its node ID, so you can tell it is your server;
- whether control requests are sealed (`control=sealed`);
- under `attempts:`, each protocol the last connection tried, in order, with how long it took and why it stopped. `stage=probe` means the app could not find or reach the server for that protocol; `stage=connect` means it reached it but the protocol did not come up; `stage=connected` means it worked and then stopped;
- under `events:`, every step of the last connection.

A protocol that fails at `probe` or `connect` on one network and works on another is almost always blocked by that network.

## 2. Look at the invite on the server

```bash
sudo ket invites show ID
```

Under **Recent activity**, the server lists what devices recently did with that invite: joined, checked in, disconnected, were refused and why (wrong access code, paused, device limit, app too old), and whether their traffic reached the OpenVPN or Hysteria 2 server. A device that joins but never shows **Reached the server over OpenVPN** is on a network that blocks OpenVPN before it gets to you. The list holds no addresses and lives only in the server's memory (see [PRIVACY.md](PRIVACY.md#kept-in-memory-only)).

## 3. Test from a network that blocks nothing

The reachability lab connects with every protocol, one at a time, and says which work. It needs Docker and runs in a few minutes:

```bash
scripts/reachability-lab.sh ubuntu@your-server --ssh-key ~/.ssh/your-key --build-openvpn
```

It creates a short-lived invite on the server over SSH and revokes it afterwards. Run it on the network that has trouble, and on one that does not, and compare.

GitHub can run it for you from its own network. Create an invite for it, store it as a repository secret, and start the **Reachability** workflow:

```bash
sudo ket invite --label reachability --devices 1 --days 30 --no-qr   # on the server
gh secret set KET_REACHABILITY_INVITE                                 # paste the ket:invite: line
gh workflow run reachability.yml
```

The workflow log names no server address. Revoke the invite (`sudo ket invites`, then `sudo ket revoke ID`) when you no longer need it.
