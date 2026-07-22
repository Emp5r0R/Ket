import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  BookOpen,
  CircleStop,
  LoaderCircle,
  MapPin,
  Power,
  Radio,
} from "lucide-react";
import { formatDuration } from "../lib/format";
import {
  autoProtocol,
  isProtocolId,
  protocolInfo,
  type ProtocolId,
  type ProtocolPreference,
} from "../lib/protocols";
import type { ClientSnapshot, EngineReadiness } from "../types";

interface ConnectionPanelProps {
  snapshot: ClientSnapshot;
  engine: EngineReadiness;
  busy: boolean;
  preference: ProtocolPreference;
  onPreferenceChange: (preference: ProtocolPreference) => void;
  onLearnMore: (protocol: ProtocolId) => void;
  onConnect: () => Promise<void>;
  onStop: () => Promise<void>;
}

const workingPhases = new Set(["probing", "connecting", "reconnecting", "disconnecting"]);
const cancellablePhases = new Set(["probing", "connecting", "reconnecting"]);

export function ConnectionPanel({
  snapshot,
  engine,
  busy,
  preference,
  onPreferenceChange,
  onLearnMore,
  onConnect,
  onStop,
}: ConnectionPanelProps) {
  const node = snapshot.node;
  if (!node) return null;
  const connected = snapshot.phase === "connected";
  const working = busy || workingPhases.has(snapshot.phase);
  const cancellable = cancellablePhases.has(snapshot.phase);
  const disconnecting = snapshot.phase === "disconnecting";
  const runtimeReady = engine.broker_available && engine.binary_available;
  const expiresIn = snapshot.access_expires_at_epoch_seconds !== null
    ? snapshot.access_expires_at_epoch_seconds - Math.floor(Date.now() / 1000)
    : null;
  const location = [node.location.city, node.location.country_name].filter(Boolean).join(", ");
  const offeredProtocols = Array.from(
    new Set(
      snapshot.available_transports
        .map((transport) => transport.protocol)
        .filter(isProtocolId),
    ),
  );
  const selectedInfo = preference === "auto" ? autoProtocol : protocolInfo(preference);
  const guideProtocol = preference === "auto" ? (offeredProtocols[0] ?? "stealth") : preference;

  return (
    <aside className="connection-panel" aria-label="Connection controls">
      <div className="node-summary">
        <div>
          <span className="section-kicker">Current node</span>
          <h2>{node.display_name}</h2>
        </div>
        <span className={`health-pill health-${node.health}`}>
          <CheckCircle2 size={14} aria-hidden="true" />
          {node.health}
        </span>
      </div>

      <div className="node-facts">
        <div>
          <MapPin size={16} aria-hidden="true" />
          <span>{location}</span>
        </div>
        <div>
          <Clock3 size={16} aria-hidden="true" />
          <span>{formatDuration(expiresIn)} access left</span>
        </div>
        <div>
          <Radio size={16} aria-hidden="true" />
          <span>{snapshot.active_transport?.display_name ?? "Automatic transport"}</span>
        </div>
      </div>

      <div className="protocol-choice">
        <div className="protocol-choice-heading">
          <label htmlFor="protocol-preference">Preferred protocol</label>
          <button type="button" onClick={() => onLearnMore(guideProtocol)}>
            <BookOpen size={14} aria-hidden="true" />
            Learn more
          </button>
        </div>
        <select
          id="protocol-preference"
          value={preference}
          disabled={working || connected}
          onChange={(event) => onPreferenceChange(event.target.value as ProtocolPreference)}
        >
          <option value="auto">Automatic</option>
          {offeredProtocols.map((protocol) => (
            <option key={protocol} value={protocol}>{protocolInfo(protocol).label}</option>
          ))}
        </select>
        <p>{selectedInfo.shortInstruction}</p>
      </div>

      {!engine.broker_available ? (
        <div className="engine-warning" role="status">
          <AlertTriangle size={17} aria-hidden="true" />
          <span>{tunnelServiceMessage(engine.status)}</span>
        </div>
      ) : !engine.binary_available ? (
        <div className="engine-warning" role="status">
          <AlertTriangle size={17} aria-hidden="true" />
          <span>Transport engines are not installed.</span>
        </div>
      ) : null}
      {snapshot.issue ? (
        <div className="inline-issue" role="alert">
          {snapshot.issue.message}
        </div>
      ) : null}

      <div className="power-control">
        <button
          type="button"
          className={`power-button ${connected ? "is-connected" : ""} ${cancellable ? "is-cancellable" : ""}`}
          onClick={() => void (connected || cancellable ? onStop() : onConnect())}
          disabled={disconnecting || (!cancellable && (working || (!connected && !runtimeReady)))}
          aria-label={cancellable ? "Cancel connection attempt" : connected ? "Disconnect tunnel" : "Connect tunnel"}
          title={cancellable ? "Cancel" : connected ? "Disconnect" : "Connect"}
        >
          {cancellable ? <CircleStop size={30} /> : working ? <LoaderCircle className="spin" size={30} /> : <Power size={30} />}
        </button>
        <strong>{phaseLabel(snapshot.phase)}</strong>
        <span>{connected ? "Restricted-network bypass active" : cancellable ? "Testing available routes" : disconnecting ? "Stopping tunnel" : "Tunnel is inactive"}</span>
      </div>
    </aside>
  );
}

function tunnelServiceMessage(status: EngineReadiness["status"]): string {
  switch (status) {
    case "permission_required":
      return "Restart Ket from the application menu to activate tunnel access.";
    case "not_installed":
      return "Tunnel service is not installed.";
    default:
      return "Tunnel service is installed but is not responding.";
  }
}

function phaseLabel(phase: ClientSnapshot["phase"]): string {
  const labels: Record<ClientSnapshot["phase"], string> = {
    disconnected: "You are being watched",
    enrolling: "Adding server",
    enrolled: "Ready",
    probing: "Testing route",
    connecting: "Connecting",
    connected: "Liberated",
    reconnecting: "Reconnecting",
    disconnecting: "Disconnecting",
    error: "Needs attention",
  };
  return labels[phase];
}
