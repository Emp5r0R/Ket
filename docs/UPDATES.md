# Updates

Every Ket release says how urgent it is, separately for the apps and for servers. Servers use that rating to tell each connecting app whether it must, should, or may update.

## Release ratings

Each release has an entry in [`crates/ket-update/releases.json`](../crates/ket-update/releases.json), which is compiled into the server:

```json
{
  "version": "1.1.0",
  "published": "2026-10-20",
  "client": "security",
  "server": "recommended",
  "platforms": ["windows"],
  "summary": "Fixes a DNS leak on Windows."
}
```

| Rating | Apps | Servers |
| --- | --- | --- |
| `optional` | A quiet "update available" note that can be hidden | Waits for `sudo ket update` |
| `recommended` | A note that can be hidden until the next version | Waits for `sudo ket update` |
| `security` | A warning that cannot be hidden. After the grace period (`security_grace_days`, 14 days from `published`) the update becomes required | Applied by the daily check with the default `KET_AUTO_UPDATE=security` |
| `critical` | Required at once | Applied by the daily check unless automatic updates are off |

`client` or `server` is left out when a release changes nothing there. A server weighs both ratings when it decides how urgent its own update is: apps only learn about a fix from servers that know the release, so an app-only security release still updates servers on their own. `platforms` limits the app rating to some apps (`android`, `linux`, `windows`, `macos`); apps from v1.0.1 and earlier do not say which platform they are, so every rating applies to them.

## What apps see

An app sends `Ket-Client: <platform>/<version>` (see [CONTROL_API.md](CONTROL_API.md#app-versions-and-update-advice)). The server looks at every release newer than the app, up to its own version, and takes the most urgent rating:

| Result | Desktop and Android |
| --- | --- |
| `current` | Nothing |
| `optional`, `recommended` | A note with a "get the update" link; "Not now" hides it until the next version |
| `security` | A warning with the date it becomes required; it stays on screen |
| `required` | The server refuses new connections and renewals (`426`). The app disconnects, keeps your server and invite, and shows where to get the update. Install it and connect again |

The check runs only after the access code or session is verified, so a stranger probing the server learns nothing about its version.

The desktop app's Settings also shows the server's version, and notes when the server is older than the app.

### Operator settings

In the server's `.env`:

| Setting | Default | Effect |
| --- | --- | --- |
| `KET_CLIENT_MINIMUM_VERSION` | none | Refuse apps older than this version, on top of the release ratings |
| `KET_CLIENT_UPDATE_ENFORCEMENT` | `enforce` | `warn` keeps serving apps whose update is required, with an urgent warning instead. Use it when your users cannot reach the download page and need the tunnel to get the update |
| `KET_CLIENT_DOWNLOAD_URL` | the GitHub releases page | An `https://` page where your users get the apps, such as your own mirror |

Apply a change with `sudo ket restart`.

## What servers do

A daily timer runs `ket update --auto`. It compares the installed release with the latest one, rates the gap with the rule above, and applies the update when `KET_AUTO_UPDATE` allows (`off`, `critical`, `security` (default), or `all`); otherwise `sudo ket status` shows what is waiting. Updates are health-checked and roll back on their own; see [SERVER_INSTALL.md](SERVER_INSTALL.md#updates) for the steps, `ket rollback`, and how v1.0.1 servers get the `ket` command.

The rule exists twice, in Rust (`ReleaseManifest::server_update_category`) and in the `ket` command (jq); both replay [`server-updates.json`](../crates/ket-update/tests/vectors/server-updates.json) so they cannot drift apart.

Automatic updates trust GitHub release tags the same way the one-command install does. Set `KET_AUTO_UPDATE=off` to review every release yourself.

## Releasing

Add the release's entry to `releases.json` in the commit that bumps the version. The `ket-update` tests fail when the newest entry does not match the workspace version, and when entries are out of order, malformed, or dated impossibly. Rate honestly: `critical` cuts off every older app immediately, including people who cannot easily download an update.
