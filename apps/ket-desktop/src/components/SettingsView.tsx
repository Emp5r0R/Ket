import { Binary, KeyRound, Laptop, Server, ShieldCheck, Trash2 } from "lucide-react";
import { endpointHost } from "../lib/format";
import type { DesktopState } from "../types";

interface SettingsViewProps {
  state: DesktopState;
  serverUrl: string;
  deviceName: string;
  busy: boolean;
  onForget: () => Promise<void>;
}

export function SettingsView({ state, serverUrl, deviceName, busy, onForget }: SettingsViewProps) {
  const rows = [
    { icon: Server, label: "Control server", value: serverUrl ? endpointHost(serverUrl) : "Not configured" },
    { icon: Laptop, label: "Device", value: deviceName || "Ket desktop" },
    { icon: Binary, label: "Transport engines", value: state.engine.binary_available ? "Available" : "Not installed" },
    { icon: ShieldCheck, label: "Tunnel service", value: state.engine.broker_available ? "Ready" : "Pending installation" },
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

      <section className="settings-section danger-section" aria-labelledby="access-heading">
        <div className="settings-heading">
          <div>
            <h2 id="access-heading">Server access</h2>
            <span>Session credentials remain in memory.</span>
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
