import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  LoaderCircle,
  MapPin,
  Power,
  Radio,
} from "lucide-react";
import { formatDuration } from "../lib/format";
import type { ClientSnapshot, EngineReadiness } from "../types";

interface ConnectionPanelProps {
  snapshot: ClientSnapshot;
  engine: EngineReadiness;
  busy: boolean;
  onConnect: () => Promise<void>;
  onStop: () => Promise<void>;
}

const workingPhases = new Set(["probing", "connecting", "reconnecting", "disconnecting"]);

export function ConnectionPanel({ snapshot, engine, busy, onConnect, onStop }: ConnectionPanelProps) {
  const node = snapshot.node;
  if (!node) return null;
  const connected = snapshot.phase === "connected";
  const working = busy || workingPhases.has(snapshot.phase);
  const runtimeReady = engine.broker_available && engine.binary_available;
  const expiresIn = snapshot.session_expires_at_epoch_seconds
    ? snapshot.session_expires_at_epoch_seconds - Math.floor(Date.now() / 1000)
    : null;
  const location = [node.location.city, node.location.country_name].filter(Boolean).join(", ");

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
          <span>{formatDuration(expiresIn)} lease</span>
        </div>
        <div>
          <Radio size={16} aria-hidden="true" />
          <span>{snapshot.active_transport?.display_name ?? "Automatic transport"}</span>
        </div>
      </div>

      {!engine.broker_available ? (
        <div className="engine-warning" role="status">
          <AlertTriangle size={17} aria-hidden="true" />
          <span>Tunnel service is not installed or unavailable.</span>
        </div>
      ) : !engine.binary_available ? (
        <div className="engine-warning" role="status">
          <AlertTriangle size={17} aria-hidden="true" />
          <span>Hysteria engine is not installed.</span>
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
          className={`power-button ${connected ? "is-connected" : ""}`}
          onClick={() => void (connected ? onStop() : onConnect())}
          disabled={working || (!connected && !runtimeReady)}
          aria-label={connected ? "Disconnect tunnel" : "Connect tunnel"}
          title={connected ? "Disconnect" : "Connect"}
        >
          {working ? <LoaderCircle className="spin" size={30} /> : <Power size={30} />}
        </button>
        <strong>{phaseLabel(snapshot.phase)}</strong>
        <span>{connected ? "Traffic is protected" : "Tunnel is inactive"}</span>
      </div>
    </aside>
  );
}

function phaseLabel(phase: ClientSnapshot["phase"]): string {
  const labels: Record<ClientSnapshot["phase"], string> = {
    disconnected: "Disconnected",
    enrolling: "Adding server",
    enrolled: "Ready",
    probing: "Testing route",
    connecting: "Connecting",
    connected: "Connected",
    reconnecting: "Reconnecting",
    disconnecting: "Disconnecting",
    error: "Needs attention",
  };
  return labels[phase];
}
