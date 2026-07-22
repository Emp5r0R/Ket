import { useEffect, useMemo, useState } from "react";
import { Binoculars, LockOpen, MapPin } from "lucide-react";
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
import { ProtocolsView } from "./components/ProtocolsView";
import { StatStrip } from "./components/StatStrip";
import { WorldMap } from "./components/WorldMap";
import {
  isProtocolId,
  type ProtocolId,
  type ProtocolPreference,
} from "./lib/protocols";

const SERVER_KEY = "ket.server-url";
const ACCESS_CODE_KEY = "ket.access-code";
const DEVICE_KEY = "ket.device-name";
const PROTOCOL_KEY = "ket.protocol-preference";
const ACCESS_EXPIRY_KEY = "ket.access-expires-at";

interface SavedEnrollment {
  serverUrl: string;
  accessCode: string;
  accessExpiresAt: number | null;
}

function clearSavedEnrollment() {
  localStorage.removeItem(SERVER_KEY);
  localStorage.removeItem(ACCESS_CODE_KEY);
  localStorage.removeItem(ACCESS_EXPIRY_KEY);
}

function loadSavedEnrollment(): SavedEnrollment {
  const savedExpiry = localStorage.getItem(ACCESS_EXPIRY_KEY);
  const expiresAt = savedExpiry === null ? null : Number(savedExpiry);
  if (expiresAt !== null && Number.isFinite(expiresAt) && expiresAt > 0 && expiresAt <= Date.now() / 1000) {
    clearSavedEnrollment();
    return { serverUrl: "", accessCode: "", accessExpiresAt: null };
  }
  const validExpiry = expiresAt !== null && Number.isFinite(expiresAt) && expiresAt > 0
    ? expiresAt
    : null;
  if (validExpiry === null) {
    localStorage.removeItem(ACCESS_CODE_KEY);
    localStorage.removeItem(ACCESS_EXPIRY_KEY);
  }
  return {
    serverUrl: localStorage.getItem(SERVER_KEY) ?? "",
    accessCode: validExpiry === null ? "" : (localStorage.getItem(ACCESS_CODE_KEY) ?? ""),
    accessExpiresAt: validExpiry,
  };
}

const emptySnapshot: ClientSnapshot = {
  phase: "disconnected",
  node: null,
  available_transports: [],
  preferred_protocol: null,
  active_transport: null,
  traffic: null,
  handshake_latency_ms: null,
  session_expires_at_epoch_seconds: null,
  access_expires_at_epoch_seconds: null,
  connected_at_epoch_seconds: null,
  reconnect_attempt: 0,
  issue: null,
  updated_at_epoch_seconds: 0,
};

const initialState: DesktopState = {
  snapshot: emptySnapshot,
  configured: false,
  engine: {
    binary_available: false,
    broker_available: false,
    mode: "privileged_broker",
    status: "unavailable",
  },
  platform: "desktop",
  version: "0.1.0",
};

export default function App() {
  const [initialEnrollment] = useState(loadSavedEnrollment);
  const [state, setState] = useState<DesktopState>(initialState);
  const [view, setView] = useState<AppView>("connection");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [localIssue, setLocalIssue] = useState<ClientIssue | null>(null);
  const [serverUrl, setServerUrl] = useState(initialEnrollment.serverUrl);
  const [savedAccessCode, setSavedAccessCode] = useState(initialEnrollment.accessCode);
  const [accessExpiresAt, setAccessExpiresAt] = useState(initialEnrollment.accessExpiresAt);
  const [deviceName, setDeviceName] = useState(
    () => localStorage.getItem(DEVICE_KEY) ?? "Ket desktop",
  );
  const [history, setHistory] = useState<TrafficPoint[]>([]);
  const [protocolPreference, setProtocolPreference] = useState<ProtocolPreference>(() => {
    const saved = localStorage.getItem(PROTOCOL_KEY);
    return saved === "auto" || (saved !== null && isProtocolId(saved)) ? saved : "auto";
  });
  const [guideProtocol, setGuideProtocol] = useState<ProtocolId>("stealth");

  const applySnapshot = (snapshot: ClientSnapshot) => {
    setState((current) => ({ ...current, snapshot, configured: snapshot.node !== null || current.configured }));
    if (
      snapshot.issue !== null ||
      ["probing", "connecting", "reconnecting", "connected"].includes(snapshot.phase)
    ) {
      setLocalIssue(snapshot.issue);
    }
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

  useEffect(() => {
    if (accessExpiresAt === null) return;
    const expire = () => {
      clearSavedEnrollment();
      setServerUrl("");
      setSavedAccessCode("");
      setAccessExpiresAt(null);
      setState((current) => ({ ...current, configured: false, snapshot: emptySnapshot }));
      void bridge.forget().catch(() => undefined);
    };
    if (accessExpiresAt * 1_000 <= Date.now()) {
      expire();
      return;
    }
    const timer = window.setInterval(() => {
      if (accessExpiresAt * 1_000 <= Date.now()) expire();
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [accessExpiresAt]);

  useEffect(() => {
    if (
      protocolPreference !== "auto" &&
      state.snapshot.available_transports.length > 0 &&
      !state.snapshot.available_transports.some(
        (transport) => transport.protocol === protocolPreference,
      )
    ) {
      setProtocolPreference("auto");
      localStorage.setItem(PROTOCOL_KEY, "auto");
    }
  }, [protocolPreference, state.snapshot.available_transports]);

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
      if (snapshot.access_expires_at_epoch_seconds !== null) {
        localStorage.setItem(ACCESS_CODE_KEY, input.accessCode);
        localStorage.setItem(
          ACCESS_EXPIRY_KEY,
          snapshot.access_expires_at_epoch_seconds.toString(),
        );
        setSavedAccessCode(input.accessCode);
        setAccessExpiresAt(snapshot.access_expires_at_epoch_seconds);
      } else {
        localStorage.removeItem(ACCESS_CODE_KEY);
        localStorage.removeItem(ACCESS_EXPIRY_KEY);
        setSavedAccessCode("");
        setAccessExpiresAt(null);
      }
      setState((current) => ({ ...current, configured: true }));
      return snapshot;
    });
  };

  const forget = async () => {
    await run(async () => {
      const snapshot = await bridge.forget();
      setState((current) => ({ ...current, configured: false }));
      clearSavedEnrollment();
      setServerUrl("");
      setSavedAccessCode("");
      setAccessExpiresAt(null);
      setHistory([]);
      setView("connection");
      return snapshot;
    });
  };

  const chooseProtocol = (preference: ProtocolPreference) => {
    setProtocolPreference(preference);
    localStorage.setItem(PROTOCOL_KEY, preference);
  };

  const openProtocolGuide = (protocol: ProtocolId) => {
    setGuideProtocol(protocol);
    setView("protocols");
  };

  const connected = state.snapshot.phase === "connected";
  const availableProtocols = useMemo(
    () => new Set(
      state.snapshot.available_transports
        .map((transport) => transport.protocol)
        .filter(isProtocolId),
    ),
    [state.snapshot.available_transports],
  );
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
    <div className={`app-shell ${connected ? "is-liberated" : "is-restricted"}`}>
      <NavRail active={view} onChange={setView} />
      <main className="app-content">
        {view === "connection" ? (
          <div className="connection-view">
            <header className="connection-header">
              <div>
                <span className="section-kicker">Secure route</span>
                <h1>{heading}</h1>
              </div>
              <div
                className={`header-status ${connected ? "is-liberated" : "is-restricted"}`}
                role="status"
                aria-live="polite"
              >
                <span className="status-symbol" aria-hidden="true">
                  {connected ? <LockOpen size={18} /> : <Binoculars size={18} />}
                </span>
                <span>{connected ? "Liberated" : "You are being watched"}</span>
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
                  preference={protocolPreference}
                  onPreferenceChange={chooseProtocol}
                  onLearnMore={openProtocolGuide}
                  onConnect={() => run(() => bridge.connect(
                    protocolPreference === "auto" ? null : protocolPreference,
                  ))}
                  onStop={() => run(() => bridge.stop())}
                />
              ) : (
                <EnrollmentPanel
                  initialServerUrl={serverUrl}
                  initialAccessCode={savedAccessCode}
                  accessExpiresAtEpochSeconds={accessExpiresAt}
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
        {view === "protocols" ? (
          <ProtocolsView
            selected={guideProtocol}
            available={availableProtocols}
            onSelect={setGuideProtocol}
          />
        ) : null}
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
