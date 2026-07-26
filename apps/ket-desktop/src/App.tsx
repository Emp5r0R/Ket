import { useEffect, useMemo, useRef, useState } from "react";
import { Binoculars, MapPin, ShieldCheck } from "lucide-react";
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
import { DiagnosticsDialog } from "./components/DiagnosticsDialog";
import {
  appendDiagnostic,
  formatDiagnosticReport,
  type ConnectionDiagnostic,
} from "./lib/diagnostics";
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
const THEME_KEY = "ket.theme";
const RESTORE_RETRY_DELAY_MS = 60_000;

export type ThemePreference = "system" | "light" | "dark";

function loadThemePreference(): ThemePreference {
  const saved = localStorage.getItem(THEME_KEY);
  return saved === "light" || saved === "dark" ? saved : "system";
}

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

export function shouldRetrySavedEnrollment(issue: ClientIssue): boolean {
  return issue.retryable || issue.code === "grant_capacity_reached";
}

export function savedEnrollmentIsTerminal(issue: ClientIssue): boolean {
  return issue.code === "grant_expired" || issue.code === "unauthorized";
}

export function savedSessionNeedsReenrollment(
  issue: ClientIssue | null,
  accessCode: string,
  accessExpiresAt: number | null,
  nowSeconds = Date.now() / 1_000,
): boolean {
  return issue?.code === "unauthorized" &&
    accessCode.length === 32 &&
    accessExpiresAt !== null &&
    accessExpiresAt > nowSeconds;
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
  version: "0.2.0",
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
  const [themePreference, setThemePreference] = useState<ThemePreference>(loadThemePreference);
  const [systemDark, setSystemDark] = useState(
    () => typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const [diagnostics, setDiagnostics] = useState<ConnectionDiagnostic[]>([]);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [surveillanceVariant, setSurveillanceVariant] = useState(0);
  const lastObservedIssue = useRef("");
  const protocolPreferenceRef = useRef(protocolPreference);

  const recordDiagnostic = (diagnostic: ConnectionDiagnostic) => {
    setDiagnostics((current) => appendDiagnostic(current, diagnostic));
  };

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
    if (snapshot.issue !== null) {
      const issueKey = [
        snapshot.updated_at_epoch_seconds,
        snapshot.phase,
        snapshot.active_transport?.id ?? "none",
        snapshot.issue.code,
        snapshot.issue.message,
      ].join(":");
      if (issueKey !== lastObservedIssue.current) {
        lastObservedIssue.current = issueKey;
        recordDiagnostic({
          recordedAtEpochMilliseconds: Date.now(),
          operation: snapshot.phase === "reconnecting" ? "reconnect" : "connection_state",
          outcome: "observed",
          phase: snapshot.phase,
          protocolPreference: protocolPreferenceRef.current,
          activeTransportId: snapshot.active_transport?.id ?? null,
          elapsedMilliseconds: null,
          issue: snapshot.issue,
        });
        if (snapshot.phase === "error") setDiagnosticsOpen(true);
      }
    }
  };

  const applyRestoredEnrollment = (snapshot: ClientSnapshot) => {
    applySnapshot(snapshot);
    setState((current) => ({ ...current, configured: true }));
    setLocalIssue(null);
    if (snapshot.access_expires_at_epoch_seconds !== null) {
      localStorage.setItem(
        ACCESS_EXPIRY_KEY,
        snapshot.access_expires_at_epoch_seconds.toString(),
      );
      setAccessExpiresAt(snapshot.access_expires_at_epoch_seconds);
    }
  };

  const handleRestoreFailure = (error: unknown) => {
    const issue = normalizeIssue(error);
    setLocalIssue(issue);
    if (savedEnrollmentIsTerminal(issue)) {
      clearSavedEnrollment();
      setServerUrl("");
      setSavedAccessCode("");
      setAccessExpiresAt(null);
    }
    return issue;
  };

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void bridge
      .state()
      .then(async (next) => {
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
        if (
          active &&
          !next.configured &&
          initialEnrollment.serverUrl !== "" &&
          initialEnrollment.accessCode.length === 32
        ) {
          try {
            const restored = await bridge.enroll({
              serverUrl: initialEnrollment.serverUrl,
              accessCode: initialEnrollment.accessCode,
              deviceName,
            });
            if (active) {
              applyRestoredEnrollment(restored);
            }
          } catch (error) {
            if (active) handleRestoreFailure(error);
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
    if (
      loading ||
      state.configured ||
      serverUrl === "" ||
      savedAccessCode.length !== 32 ||
      accessExpiresAt === null
    ) {
      return;
    }
    let active = true;
    let attempting = false;
    let timer = 0;
    const restore = async () => {
      if (!active || attempting) return;
      attempting = true;
      try {
        const snapshot = await bridge.enroll({
          serverUrl,
          accessCode: savedAccessCode,
          deviceName,
        });
        if (active) applyRestoredEnrollment(snapshot);
      } catch (error) {
        if (!active) return;
        const issue = handleRestoreFailure(error);
        if (shouldRetrySavedEnrollment(issue)) {
          timer = window.setTimeout(() => void restore(), RESTORE_RETRY_DELAY_MS);
        }
      } finally {
        attempting = false;
      }
    };
    timer = window.setTimeout(() => void restore(), RESTORE_RETRY_DELAY_MS);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [accessExpiresAt, deviceName, loading, savedAccessCode, serverUrl, state.configured]);

  useEffect(() => {
    protocolPreferenceRef.current = protocolPreference;
  }, [protocolPreference]);

  useEffect(() => {
    if (state.snapshot.phase === "connected") {
      setSurveillanceVariant(0);
      return;
    }
    const timer = window.setInterval(() => {
      setSurveillanceVariant((current) => (current + 1) % 2);
    }, 5_200);
    return () => window.clearInterval(timer);
  }, [state.snapshot.phase]);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const update = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    setSystemDark(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  useEffect(() => {
    if (!state.configured) return;
    const timer = window.setInterval(() => {
      void bridge.refresh().then(applySnapshot).catch(() => undefined);
    }, 10_000);
    return () => window.clearInterval(timer);
  }, [state.configured]);

  useEffect(() => {
    if (
      !state.configured ||
      !savedSessionNeedsReenrollment(
        state.snapshot.issue,
        savedAccessCode,
        accessExpiresAt,
      )
    ) {
      return;
    }
    let active = true;
    void bridge.forget()
      .catch(() => undefined)
      .finally(() => {
        if (active) {
          setState((current) => ({ ...current, configured: false, snapshot: emptySnapshot }));
        }
      });
    return () => {
      active = false;
    };
  }, [accessExpiresAt, savedAccessCode, state.configured, state.snapshot.issue]);

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

  const run = async (
    operationName: "enroll" | "connect" | "stop" | "forget",
    operation: () => Promise<ClientSnapshot>,
    preferredProtocol: ProtocolPreference = protocolPreferenceRef.current,
  ) => {
    const startedAt = performance.now();
    setBusy(true);
    setLocalIssue(null);
    recordDiagnostic({
      recordedAtEpochMilliseconds: Date.now(),
      operation: operationName,
      outcome: "started",
      phase: state.snapshot.phase,
      protocolPreference: preferredProtocol,
      activeTransportId: state.snapshot.active_transport?.id ?? null,
      elapsedMilliseconds: null,
      issue: null,
    });
    try {
      const snapshot = await operation();
      applySnapshot(snapshot);
      recordDiagnostic({
        recordedAtEpochMilliseconds: Date.now(),
        operation: operationName,
        outcome: "succeeded",
        phase: snapshot.phase,
        protocolPreference: preferredProtocol,
        activeTransportId: snapshot.active_transport?.id ?? null,
        elapsedMilliseconds: performance.now() - startedAt,
        issue: null,
      });
    } catch (error) {
      const issue = normalizeIssue(error);
      setLocalIssue(issue);
      recordDiagnostic({
        recordedAtEpochMilliseconds: Date.now(),
        operation: operationName,
        outcome: "failed",
        phase: state.snapshot.phase,
        protocolPreference: preferredProtocol,
        activeTransportId: state.snapshot.active_transport?.id ?? null,
        elapsedMilliseconds: performance.now() - startedAt,
        issue,
      });
      if (operationName !== "enroll") setDiagnosticsOpen(true);
    } finally {
      setBusy(false);
    }
  };

  const enroll = async (input: EnrollmentInput) => {
    setServerUrl(input.serverUrl.trim());
    setDeviceName(input.deviceName.trim());
    await run("enroll", async () => {
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
    await run("forget", async () => {
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

  const chooseTheme = (preference: ThemePreference) => {
    setThemePreference(preference);
    if (preference === "system") {
      localStorage.removeItem(THEME_KEY);
    } else {
      localStorage.setItem(THEME_KEY, preference);
    }
  };

  const openProtocolGuide = (protocol: ProtocolId) => {
    setGuideProtocol(protocol);
    setView("protocols");
  };

  const connected = state.snapshot.phase === "connected";
  const dark = themePreference === "dark" || (themePreference === "system" && systemDark);
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
  const disconnectedStatus = surveillanceVariant === 0
    ? "You are being watched"
    : "You are under surveillance";
  const diagnosticReport = useMemo(() => formatDiagnosticReport({
    platform: state.platform,
    version: state.version,
    engineStatus: state.engine.status,
    currentPhase: state.snapshot.phase,
    nodeId: state.snapshot.node?.node_id ?? null,
    protocolPreference,
    activeTransportId: state.snapshot.active_transport?.id ?? null,
  }, diagnostics), [diagnostics, protocolPreference, state]);

  if (loading) {
    return (
      <main className={`boot-screen ${dark ? "theme-dark" : ""}`}>
        <img src="/ket-mark.svg" alt="Ket" />
        <span>Starting Ket</span>
      </main>
    );
  }

  return (
    <div className={`app-shell ${dark ? "theme-dark" : "theme-light"} ${connected ? "is-liberated" : "is-restricted"}`}>
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
                  {connected ? <ShieldCheck size={18} /> : <Binoculars size={18} />}
                </span>
                <span className="status-copy">
                  <strong>{connected ? "Liberated" : disconnectedStatus}</strong>
                  {!connected ? <small>Connect to get liberated</small> : null}
                </span>
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
                  onConnect={() => run(
                    "connect",
                    () => bridge.connect(protocolPreference === "auto" ? null : protocolPreference),
                    protocolPreference,
                  )}
                  onStop={() => run("stop", () => bridge.stop(), protocolPreference)}
                  onOpenDiagnostics={() => setDiagnosticsOpen(true)}
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
            themePreference={themePreference}
            onThemeChange={chooseTheme}
            onForget={forget}
          />
        ) : null}
      </main>
      <DiagnosticsDialog
        open={diagnosticsOpen}
        report={diagnosticReport}
        onClose={() => setDiagnosticsOpen(false)}
      />
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
