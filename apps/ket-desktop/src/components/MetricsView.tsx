import { Cpu, Database, Gauge, Radio, Timer, Users } from "lucide-react";
import { formatBytes, formatDuration } from "../lib/format";
import type { ClientSnapshot, TrafficPoint } from "../types";
import { formatRate, latestRate, peakRate, throughput } from "../lib/throughput";
import { AllowanceSection } from "./AllowanceSection";
import { TrafficChart } from "./TrafficChart";

interface MetricsViewProps {
  snapshot: ClientSnapshot;
  history: TrafficPoint[];
}

export function MetricsView({ snapshot, history }: MetricsViewProps) {
  const node = snapshot.node;
  const traffic = snapshot.traffic;
  const rates = throughput(history);
  const latest = latestRate(history);
  const metrics = [
    {
      label: "Transport",
      value: snapshot.active_transport?.display_name ?? "Automatic",
      icon: Radio,
    },
    {
      label: "Handshake",
      value:
        snapshot.handshake_latency_ms == null ? "--" : `${snapshot.handshake_latency_ms} ms`,
      icon: Gauge,
    },
    {
      label: "CPU load",
      value: node?.cpu_load_percent == null ? "--" : `${node.cpu_load_percent.toFixed(1)}%`,
      icon: Cpu,
    },
    {
      label: "Memory",
      value:
        node?.memory_used_bytes == null || node.memory_total_bytes == null
          ? "--"
          : `${formatBytes(node.memory_used_bytes)} / ${formatBytes(node.memory_total_bytes)}`,
      icon: Database,
    },
    {
      label: "Sessions",
      value: node ? `${node.active_sessions} / ${node.session_capacity}` : "--",
      icon: Users,
    },
    {
      label: "Uptime",
      value: formatDuration(node?.uptime_seconds),
      icon: Timer,
    },
  ];

  return (
    <div className="metrics-view">
      <header className="view-header">
        <div>
          <span className="section-kicker">Live telemetry</span>
          <h1>Network metrics</h1>
        </div>
        <span className={`status-dot status-${node?.health ?? "idle"}`}>
          {node?.health ?? "No node"}
        </span>
      </header>

      <section className="metric-summary" aria-label="Node telemetry">
        {metrics.map(({ label, value, icon: Icon }) => (
          <div className="summary-item" key={label}>
            <Icon size={18} aria-hidden="true" />
            <span>{label}</span>
            <strong>{value}</strong>
          </div>
        ))}
      </section>

      <div className="chart-grid-layout">
        {(["received", "sent"] as const).map((metric) => (
          <section className="chart-section" key={metric}>
            <div className="chart-heading">
              <div>
                <span>{metric === "received" ? "Download" : "Upload"}</span>
                <strong>{formatRate(latest?.[metric])}</strong>
              </div>
              <span className={`chart-legend legend-${metric}`}>
                {traffic?.available
                  ? `${formatBytes(metric === "received" ? traffic.bytes_received : traffic.bytes_sent)} in total`
                  : "No traffic yet"}
              </span>
            </div>
            <TrafficChart rates={rates} metric={metric} />
            <div className="chart-footer">
              <span>Peak {formatRate(rates.length ? peakRate(rates, metric) : null)}</span>
              <span>Last {chartSpan(history)}</span>
            </div>
          </section>
        ))}
      </div>

      <section className="capacity-section">
        <div className="capacity-copy">
          <div>
            <span>Node capacity</span>
            <strong>{node ? `${Math.round(node.capacity_percent)}%` : "--"}</strong>
          </div>
          <span>{node ? `${node.session_capacity - node.active_sessions} session slots available` : "No node data"}</span>
        </div>
        <div className="capacity-track" aria-hidden="true">
          <span style={{ width: `${Math.min(Math.max(node?.capacity_percent ?? 0, 0), 100)}%` }} />
        </div>
      </section>

      {snapshot.allowance && <AllowanceSection allowance={snapshot.allowance} />}
    </div>
  );
}

/** How much time the chart covers, such as "2 min". */
function chartSpan(history: TrafficPoint[]): string {
  const seconds = history.length > 1 ? history[history.length - 1].at - history[0].at : 0;
  if (seconds < 60) return `${Math.max(seconds, 0)} s`;
  return `${Math.round(seconds / 60)} min`;
}
