import { useEffect, useState } from "react";
import { Copy, RefreshCw, Share2, Square } from "lucide-react";
import { devicesText, networkLabel, type Sharing } from "../lib/sharing";

interface SharePanelProps {
  sharing: Sharing;
  connected: boolean;
}

/**
 * Settings for sharing this computer's Ket connection with nearby devices
 * (a phone on the same Wi-Fi, or one joined to this computer's hotspot).
 * Off until the user starts it; every start draws a new password.
 */
export function SharePanel({ sharing, connected }: SharePanelProps) {
  const { networks, view, status, notice, busy, refreshNetworks, start, stop } = sharing;
  const [chosen, setChosen] = useState("");
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    void refreshNetworks();
  }, [refreshNetworks]);

  const selected = networks.some((network) => network.address === chosen)
    ? chosen
    : networks[0]?.address ?? "";

  const copy = async (label: string, text: string) => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(text);
      setCopied(label);
    } catch {
      setCopied(null);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="share-heading">
      <div className="settings-heading">
        <div>
          <h2 id="share-heading">Share with nearby devices</h2>
          <span>Let a phone or another computer use this connection.</span>
        </div>
        <Share2 size={19} aria-hidden="true" />
      </div>
      <div className="share-panel">
        {view === null ? (
          <>
            <p className="share-explainer">
              Ket opens a proxy on the network you choose, protected by a new password each time.
              Devices on that network set it as their proxy and browse through your Ket connection.
              Use it on networks you trust, such as this computer&apos;s own hotspot: the link between
              them and this computer is not encrypted by Ket, though HTTPS sites stay encrypted.
            </p>
            {networks.length === 0 ? (
              <p className="share-note">
                This computer is not on a local network it can share on. Join a Wi-Fi network or
                turn on its hotspot, then
                {" "}
                <button type="button" className="link-button" onClick={() => void refreshNetworks()}>
                  look again
                </button>
                .
              </p>
            ) : (
              <div className="share-start">
                <label>
                  <span>Network</span>
                  <select value={selected} onChange={(event) => setChosen(event.target.value)}>
                    {networks.map((network) => (
                      <option key={network.address} value={network.address}>
                        {networkLabel(network)}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  type="button"
                  className="icon-button"
                  aria-label="Look for networks again"
                  onClick={() => void refreshNetworks()}
                >
                  <RefreshCw size={15} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  className="secondary-button"
                  disabled={busy || !connected || selected === ""}
                  onClick={() => void start(selected)}
                >
                  <Share2 size={16} aria-hidden="true" />
                  Start sharing
                </button>
              </div>
            )}
            {!connected && networks.length > 0 ? (
              <p className="share-note">Connect Ket first; sharing stops when you disconnect.</p>
            ) : null}
          </>
        ) : (
          <div className="share-active">
            <img
              className="share-qr"
              src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(view.qr_svg)}`}
              alt="QR code with the proxy address and password"
              width={168}
              height={168}
            />
            <div className="share-details">
              <p className="share-status" role="status" aria-live="polite">
                <strong>{devicesText(status?.devices ?? 0)}</strong>
                {status !== null && !status.tunnel ? (
                  <small> · Paused while Ket reconnects</small>
                ) : null}
              </p>
              <dl>
                <dt>Proxy (HTTP or SOCKS5)</dt>
                <dd><code>{view.details.network.address}</code></dd>
                <dt>Port</dt>
                <dd><code>{view.details.port}</code></dd>
                <dt>User name</dt>
                <dd><code>{view.details.username}</code></dd>
                <dt>Password</dt>
                <dd>
                  <code>{view.details.password}</code>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label="Copy the password"
                    onClick={() => void copy("password", view.details.password)}
                  >
                    <Copy size={14} aria-hidden="true" />
                  </button>
                </dd>
              </dl>
              <p className="share-note">
                On the other device, open its Wi-Fi settings, set the proxy to Manual with this
                address and port, and sign in when the browser asks. Proxy apps can scan the code or
                {" "}
                <button type="button" className="link-button" onClick={() => void copy("link", view.proxy_url)}>
                  copy the proxy link
                </button>
                .
                {copied !== null ? <span role="status"> Copied.</span> : null}
              </p>
              {status !== null && status.refused_sign_ins > 0 ? (
                <p className="share-note share-warning">
                  {status.refused_sign_ins === 1
                    ? "1 sign-in with a wrong password was refused."
                    : `${status.refused_sign_ins} sign-ins with a wrong password were refused.`}
                  {" "}Stop and start sharing again for a new password if that was not you.
                </p>
              ) : null}
              <button type="button" className="danger-button" disabled={busy} onClick={() => void stop()}>
                <Square size={15} aria-hidden="true" />
                Stop sharing
              </button>
            </div>
          </div>
        )}
        {notice !== null ? <p className="settings-bypass-refusal" role="alert">{notice}</p> : null}
      </div>
    </section>
  );
}
