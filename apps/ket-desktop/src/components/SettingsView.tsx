import {
  Binary,
  KeyRound,
  ListChecks,
  Laptop,
  Monitor,
  Moon,
  Scissors,
  Server,
  ShieldCheck,
  Sun,
  Trash2,
} from "lucide-react";
import { endpointHost } from "../lib/format";
import { compareVersions } from "../lib/updates";
import type { ThemePreference } from "../App";
import { protocolInfo, type ProtocolId } from "../lib/protocols";
import type { DesktopState } from "../types";
import { BypassListEditor } from "./BypassListEditor";
import { SharePanel } from "./SharePanel";
import type { Sharing } from "../lib/sharing";

interface SettingsViewProps {
  state: DesktopState;
  serverUrl: string;
  deviceName: string;
  busy: boolean;
  themePreference: ThemePreference;
  onThemeChange: (preference: ThemePreference) => void;
  splitTlsHello: boolean;
  onSplitTlsHelloChange: (enabled: boolean) => void;
  automaticExcluded: ProtocolId[];
  onAutomaticProtocolChange: (protocol: ProtocolId, included: boolean) => void;
  bypass: string[];
  checkBypassEntry: (entry: string) => Promise<string>;
  onBypassChange: (entries: string[]) => void;
  sharing: Sharing;
  connected: boolean;
  onForget: () => Promise<void>;
}

const themeOptions = [
  { id: "system", label: "System", icon: Monitor },
  { id: "light", label: "Light", icon: Sun },
  { id: "dark", label: "Dark", icon: Moon },
] satisfies { id: ThemePreference; label: string; icon: typeof Monitor }[];

export function SettingsView({
  state,
  serverUrl,
  deviceName,
  busy,
  themePreference,
  onThemeChange,
  splitTlsHello,
  onSplitTlsHelloChange,
  automaticExcluded,
  onAutomaticProtocolChange,
  bypass,
  checkBypassEntry,
  onBypassChange,
  sharing,
  connected,
  onForget,
}: SettingsViewProps) {
  const offered = [
    ...new Set(state.snapshot.available_transports.map((transport) => transport.protocol)),
  ];
  const tunnelStatus = state.engine.broker_available
    ? "Ready"
    : state.engine.status === "not_installed"
      ? "Not installed"
      : state.engine.status === "permission_required"
        ? "Restart required"
        : "Unavailable";
  const serverVersion = state.snapshot.server_version;
  const serverVersionText = serverVersion === null
    ? "Unknown"
    : compareVersions(serverVersion, state.version) < 0
      ? `Ket ${serverVersion} (older than this app; its operator can run: sudo ket update)`
      : `Ket ${serverVersion}`;
  const rows = [
    { icon: Server, label: "Control server", value: serverUrl ? endpointHost(serverUrl) : "Not configured" },
    { icon: Server, label: "Server version", value: serverVersionText },
    { icon: Laptop, label: "Device", value: deviceName || "Ket desktop" },
    { icon: Binary, label: "Transport engines", value: state.engine.binary_available ? "Available" : "Not installed" },
    { icon: ShieldCheck, label: "Tunnel service", value: tunnelStatus },
  ];

  return (
    <div className="settings-view">
      <header className="view-header">
        <div>
          <span className="section-kicker">Local configuration</span>
          <h1>Settings</h1>
        </div>
        <span className="version-label">Ket {state.version}</span>
      </header>

      <section className="settings-section" aria-labelledby="runtime-heading">
        <div className="settings-heading">
          <h2 id="runtime-heading">Runtime</h2>
          <span>{state.platform}</span>
        </div>
        <div className="settings-rows">
          {rows.map(({ icon: Icon, label, value }) => (
            <div className="settings-row" key={label}>
              <span className="settings-row-icon"><Icon size={18} aria-hidden="true" /></span>
              <span>{label}</span>
              <strong>{value}</strong>
            </div>
          ))}
        </div>
      </section>

      <section className="settings-section" aria-labelledby="appearance-heading">
        <div className="settings-heading">
          <div>
            <h2 id="appearance-heading">Appearance</h2>
            <span>Theme</span>
          </div>
        </div>
        <div className="theme-segment" role="radiogroup" aria-label="Color theme">
          {themeOptions.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              className={themePreference === id ? "is-active" : ""}
              role="radio"
              aria-checked={themePreference === id}
              onClick={() => onThemeChange(id)}
            >
              <Icon size={16} aria-hidden="true" />
              <span>{label}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="settings-section" aria-labelledby="connection-heading">
        <div className="settings-heading">
          <div>
            <h2 id="connection-heading">Connection</h2>
            <span>Applies from the next connection.</span>
          </div>
        </div>
        <label className="settings-toggle">
          <span className="settings-row-icon"><Scissors size={18} aria-hidden="true" /></span>
          <span>
            <strong>Split the TLS greeting</strong>
            <small>
              Sends the first message of VLESS + REALITY and HTTPS Stealth in small pieces,
              so a filter that reads the server name from one packet misses it. Try it if
              those two are blocked; it adds a fraction of a second to connecting.
            </small>
          </span>
          <input
            type="checkbox"
            checked={splitTlsHello}
            onChange={(event) => onSplitTlsHelloChange(event.target.checked)}
          />
        </label>
        {offered.length > 1 ? (
          <fieldset className="settings-checklist">
            <legend>
              <span className="settings-row-icon"><ListChecks size={18} aria-hidden="true" /></span>
              <span>
                <strong>Protocols Automatic tries</strong>
                <small>
                  Leave out ones that never work for you. Choosing a protocol by hand still
                  uses it, and if you leave out every one, Automatic tries them all.
                </small>
              </span>
            </legend>
            {offered.map((protocol) => (
              <label key={protocol}>
                <input
                  type="checkbox"
                  checked={!automaticExcluded.includes(protocol)}
                  onChange={(event) => onAutomaticProtocolChange(protocol, event.target.checked)}
                />
                {protocolInfo(protocol).label}
              </label>
            ))}
          </fieldset>
        ) : null}
        <BypassListEditor entries={bypass} check={checkBypassEntry} onChange={onBypassChange} />
      </section>

      <SharePanel sharing={sharing} connected={connected} />

      <section className="settings-section danger-section" aria-labelledby="access-heading">
        <div className="settings-heading">
          <div>
            <h2 id="access-heading">Server access</h2>
            <span>The saved profile is removed when its access time expires.</span>
          </div>
          <KeyRound size={19} aria-hidden="true" />
        </div>
        <button
          type="button"
          className="danger-button"
          disabled={busy || !state.configured}
          onClick={() => void onForget()}
        >
          <Trash2 size={17} aria-hidden="true" />
          Forget server
        </button>
      </section>
    </div>
  );
}
