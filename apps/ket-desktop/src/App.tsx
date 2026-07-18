import { useEffect, useMemo, useState } from "react";
import { CircleHelp, MapPin } from "lucide-react";
import { bridge } from "./lib/bridge";
import type {
  ClientIssue,
  ClientSnapshot,
  DesktopState,
  EnrollmentInput,
  TrafficPoint,
} from "./types";
import { ConnectionPanel } from "./components/ConnectionPanel";
import { EnrollmentPanel } from "./components/EnrollmentPanel";
import { MetricsView } from "./components/MetricsView";
import { NavRail, type AppView } from "./components/NavRail";
import { SettingsView } from "./components/SettingsView";
import { StatStrip } from "./components/StatStrip";
import { WorldMap } from "./components/WorldMap";

const SERVER_KEY = "ket.server-url";
const DEVICE_KEY = "ket.device-name";

const emptySnapshot: ClientSnapshot = {
  phase: "disconnected",
  node: null,
  active_transport: null,
  traffic: null,
  handshake_latency_ms: null,
  session_expires_at_epoch_seconds: null,
  connected_at_epoch_seconds: null,
  reconnect_attempt: 0,
  issue: null,
  updated_at_epoch_seconds: 0,
};

const initialState: DesktopState = {
  snapshot: emptySnapshot,
  configured: false,
  engine: { binary_available: false, broker_available: false, mode: "privileged_broker" },
  platform: "desktop",
  version: "0.1.0",
};

export default function App() {
  const [state, setState] = useState<DesktopState>(initialState);
  const [view, setView] = useState<AppView>("connection");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [localIssue, setLocalIssue] = useState<ClientIssue | null>(null);
  const [serverUrl, setServerUrl] = useState(() => localStorage.getItem(SERVER_KEY) ?? "");
  const [deviceName, setDeviceName] = useState(
    () => localStorage.getItem(DEVICE_KEY) ?? "Ket desktop",
  );
  const [history, setHistory] = useState<TrafficPoint[]>([]);

  const applySnapshot = (snapshot: ClientSnapshot) => {
    setState((current) => ({ ...current, snapshot, configured: snapshot.node !== null || current.configured }));
    if (snapshot.traffic?.available) {
      const point = {
        at: snapshot.traffic.observed_at_epoch_seconds,
        sent: snapshot.traffic.bytes_sent,
        received: snapshot.traffic.bytes_received,
      };
      setHistory((current) => {
        if (current.at(-1)?.at === point.at) return current;
        return [...current, point].slice(-24);
      });
    }
  };

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void bridge
      .state()
      .then((next) => {
        if (active) {
          setState(next);
          const traffic = next.snapshot.traffic;
          if (traffic?.available) {
            setHistory([
              {
                at: traffic.observed_at_epoch_seconds,
                sent: traffic.bytes_sent,
                received: traffic.bytes_received,
              },
            ]);
          }
        }
      })
      .catch((error) => {
        if (active) setLocalIssue(normalizeIssue(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    void bridge.subscribe((snapshot) => {
      if (active) applySnapshot(snapshot);
    }).then((stop) => {
      unsubscribe = stop;
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    if (!state.configured) return;
    const timer = window.setInterval(() => {
      void bridge.refresh().then(applySnapshot).catch(() => undefined);
    }, 10_000);
    return () => window.clearInterval(timer);
  }, [state.configured]);

  const run = async (operation: () => Promise<ClientSnapshot>) => {
    setBusy(true);
    setLocalIssue(null);
    try {
      applySnapshot(await operation());
    } catch (error) {
      setLocalIssue(normalizeIssue(error));
    } finally {
      setBusy(false);
    }
  };

  const enroll = async (input: EnrollmentInput) => {
    setServerUrl(input.serverUrl.trim());
    setDeviceName(input.deviceName.trim());
    await run(async () => {
      const snapshot = await bridge.enroll(input);
      localStorage.setItem(SERVER_KEY, input.serverUrl.trim());
      localStorage.setItem(DEVICE_KEY, input.deviceName.trim());
      setState((current) => ({ ...current, configured: true }));
      return snapshot;
    });
  };

  const forget = async () => {
    await run(async () => {
      const snapshot = await bridge.forget();
      setState((current) => ({ ...current, configured: false }));
      setHistory([]);
      setView("connection");
      return snapshot;
    });
  };

  const connected = state.snapshot.phase === "connected";
  const heading = useMemo(() => {
    const location = state.snapshot.node?.location;
    if (!location) return "Choose your node";
    return location.city ? `${location.city}, ${location.country_name}` : location.country_name;
  }, [state.snapshot.node]);

  if (loading) {
    return (
      <main className="boot-screen">
        <img src="/ket-mark.svg" alt="Ket" />
        <span>Starting Ket</span>
      </main>
    );
  }

  return (
    <div className="app-shell">
      <NavRail active={view} onChange={setView} />
      <main className="app-content">
        {view === "connection" ? (
          <div className="connection-view">
            <header className="connection-header">
              <div>
                <span className="section-kicker">Secure route</span>
                <h1>{heading}</h1>
              </div>
              <div className="header-status">
                <span className={`connection-dot ${connected ? "is-connected" : ""}`} />
                <span>{connected ? "Protected" : state.configured ? "Ready" : "No server"}</span>
                <button className="icon-button" type="button" title="Connection status" aria-label="Connection status">
                  <CircleHelp size={18} />
                </button>
              </div>
            </header>
            <div className="connection-layout">
              <section className="map-stage" aria-label="Server map">
                <WorldMap location={state.snapshot.node?.location ?? null} connected={connected} />
                <div className="map-caption">
                  <MapPin size={16} aria-hidden="true" />
                  <span>{state.snapshot.node?.display_name ?? "No server selected"}</span>
                  <span>{state.snapshot.node?.location.country_code ?? "--"}</span>
                </div>
                <StatStrip snapshot={state.snapshot} />
              </section>
              {state.configured && state.snapshot.node ? (
                <ConnectionPanel
                  snapshot={{
                    ...state.snapshot,
                    issue: localIssue ?? state.snapshot.issue,
                  }}
                  engine={state.engine}
                  busy={busy}
                  onConnect={() => run(() => bridge.connect())}
                  onStop={() => run(() => bridge.stop())}
                />
              ) : (
                <EnrollmentPanel
                  initialServerUrl={serverUrl}
                  initialDeviceName={deviceName}
                  busy={busy}
                  issue={localIssue}
                  onEnroll={enroll}
                />
              )}
            </div>
          </div>
        ) : null}
        {view === "metrics" ? <MetricsView snapshot={state.snapshot} history={history} /> : null}
        {view === "settings" ? (
          <SettingsView
            state={state}
            serverUrl={serverUrl}
            deviceName={deviceName}
            busy={busy}
            onForget={forget}
          />
        ) : null}
      </main>
    </div>
  );
}

function normalizeIssue(error: unknown): ClientIssue {
  if (typeof error === "object" && error !== null && "message" in error) {
    const candidate = error as Partial<ClientIssue>;
    return {
      code: typeof candidate.code === "string" ? candidate.code : "desktop_failure",
      message: typeof candidate.message === "string" ? candidate.message : "The operation failed.",
      retryable: candidate.retryable === true,
    };
  }
  return {
    code: "desktop_failure",
    message: typeof error === "string" ? error : "The operation failed.",
    retryable: false,
  };
}
