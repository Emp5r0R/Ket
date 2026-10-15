<p align="center">
  <img src="assets/ket-mark.svg" width="104" height="104" alt="Ket Psi wavefront mark">
</p>

# Ket

Ket keeps you connected on networks that block or slow down VPNs. You run your own small server, share an invite with the people you trust, and the Ket app finds a way through.

## 1. Set up a server

Rent any Linux VPS (amd64 or arm64), then run:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash
```

Have a domain? Add it at the end. Ket works out on its own whether it points straight at the server or goes through Cloudflare:

```bash
curl -fsSL https://raw.githubusercontent.com/Emp5r0R/Ket/main/install.sh | sudo bash -s -- vpn.example.com
```

Before you run it, allow these ports in your cloud provider's firewall:

| Protocol | Ports |
| --- | --- |
| TCP | 80, 443, 9443 |
| UDP | 443 |
| TCP and UDP | 20000-20031 |

When it finishes, you get an **invite** and a QR code.

## 2. Connect

1. Install the Ket app for Linux, Windows, or Android from the [latest release](https://github.com/Emp5r0R/Ket/releases/latest). On Android, take the `android-arm64` file; if your phone won't install it, take `android-universal`.
2. Paste the invite or scan its QR code.
3. Tap **Connect**.

## 3. Look after your server

```bash
sudo ket invite     # make another invite (it asks who it's for)
sudo ket status     # check that everything is running
sudo ket doctor     # something wrong? it finds the problem and tells you the fix
sudo ket help       # updates, backups, uninstalling, and more
```

Security fixes install themselves. The Ket app tells you when it needs updating.

## Learn more

- [Server setup in detail](docs/SERVER_INSTALL.md): supported systems, domains and Cloudflare, the `ket` command, uninstalling.
- [How updates work](docs/UPDATES.md): what gets updated automatically, and when an app must update.
- [Features and transports](docs/FEATURES.md): what Ket does and how each connection method is tested.
- [How invites work](docs/INVITE_CAPSULE.md) and the [architecture](docs/ARCHITECTURE.md).
- [Running the server by hand](docs/MANUAL_DEPLOYMENT.md) and [developing Ket](docs/DEVELOPMENT.md).

## License

Ket is licensed under the [Apache License 2.0](LICENSE). Third-party notices are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
