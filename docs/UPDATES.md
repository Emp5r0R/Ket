# Updates

Every Ket release says how urgent it is, separately for the apps and for servers. Servers use that rating to tell each connecting app whether it must, should, or may update.

## Release ratings

Each release has an entry in [`crates/ket-update/releases.json`](../crates/ket-update/releases.json), which is compiled into the server:

```json
{
  "version": "1.1.0",
  "published": "2026-10-20",
  "client": "security",
  "apps": { "android": "optional", "windows": "security" },
  "server": "recommended",
  "summary": "Fixes a DNS leak on Windows."
}
```

| Rating | Apps | Servers |
| --- | --- | --- |
| `optional` | A quiet "update available" note that can be hidden | Waits for `sudo ket update` |
| `recommended` | A note that can be hidden until the next version | Waits for `sudo ket update` |
| `security` | A warning that cannot be hidden. After the grace period (`security_grace_days`, 14 days from `published`) the update becomes required | Applied by the daily check with the default `KET_AUTO_UPDATE=security` |
| `critical` | Required at once | Applied by the daily check unless automatic updates are off |

`client` or `server` is left out when a release changes nothing there. A server weighs both ratings when it decides how urgent its own update is: apps only learn about a fix from servers that know the release, so an app-only security release still updates servers on their own.

When the apps are not all affected alike, `apps` rates each one (`android`, `linux`, `windows`, `macos`); apps it leaves out are unchanged by the release. `client` must then be the most urgent of those ratings: servers rate their own update from it, and apps from v1.0.1 and earlier, which do not say which platform they are, get it. Without `apps`, `client` applies to every app.

### Choosing a rating

Rate each app by the worst thing the release fixes for it:

| The release, for this app | Rating |
| --- | --- |
| Fixes a bug that stops the app working, or the server can no longer serve older apps | `critical` |
| Fixes a serious security problem, such as leaking traffic, DNS, or credentials | `security` |
| Fixes bugs people notice, or a minor security issue | `recommended` |
| Adds features or polish only | `optional`, or `recommended` when most people will want it |

`critical` cuts off every older app at once, including people who cannot easily download an update, so use it only when the old app is unusable anyway. The summary is shown on every platform, so name the platform when a fix affects only some.

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

`sudo ket update --check` and `sudo ket status` also say what the waiting update asks of each app, such as `android optional, linux security`, so you know what your users will see once you apply it.

The rules exist twice, in Rust (`ReleaseManifest::server_update_category` and `app_update_category`) and in the `ket` command (jq); both replay [`server-updates.json`](../crates/ket-update/tests/vectors/server-updates.json) and [`app-updates.json`](../crates/ket-update/tests/vectors/app-updates.json) so they cannot drift apart.

Automatic updates trust GitHub release tags the same way the one-command install does. Set `KET_AUTO_UPDATE=off` to review every release yourself.

## Releasing

Add the release's entry to `releases.json` in the commit that bumps the version, rated with the guide above. The `ket-update` tests fail when the newest entry does not match the workspace version, when `client` is not the most urgent `apps` rating, and when entries are out of order, malformed, or dated impossibly.
