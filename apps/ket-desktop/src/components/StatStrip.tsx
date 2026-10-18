import { ArrowDown, ArrowUp, Gauge, Radio } from "lucide-react";
import { formatBytes, formatLatency } from "../lib/format";
import { formatRate, latestRate } from "../lib/throughput";
import type { ClientSnapshot, TrafficPoint } from "../types";

interface StatStripProps {
  snapshot: ClientSnapshot;
  history: TrafficPoint[];
}

interface Stat {
  label: string;
  value: string;
  detail?: string;
  icon: typeof ArrowDown;
  tone: string;
}

export function StatStrip({ snapshot, history }: StatStripProps) {
  const node = snapshot.node;
  const traffic = snapshot.traffic?.available ? snapshot.traffic : null;
  const live = snapshot.phase === "connected" ? latestRate(history) : null;
  const stats: Stat[] = [
    {
      label: "Download",
      value: traffic ? formatRate(live?.received) : "--",
      detail: traffic ? `${formatBytes(traffic.bytes_received)} total` : undefined,
      icon: ArrowDown,
      tone: "green",
    },
    {
      label: "Upload",
      value: traffic ? formatRate(live?.sent) : "--",
      detail: traffic ? `${formatBytes(traffic.bytes_sent)} total` : undefined,
      icon: ArrowUp,
      tone: "blue",
    },
    {
      label: "Latency",
      value: formatLatency(snapshot.handshake_latency_ms),
      icon: Radio,
      tone: "coral",
    },
    {
      label: "Node load",
      value: node ? `${Math.round(node.capacity_percent)}%` : "--",
      icon: Gauge,
      tone: "ink",
    },
  ];

  return (
    <div className="stat-strip" aria-label="Connection metrics">
      {stats.map(({ label, value, detail, icon: Icon, tone }) => (
        <div className="stat-item" key={label}>
          <span className={`stat-icon tone-${tone}`}>
            <Icon size={18} aria-hidden="true" />
          </span>
          <span className="stat-copy">
            <span>{label}</span>
            <strong>{value}</strong>
            {detail ? <small>{detail}</small> : null}
          </span>
        </div>
      ))}
    </div>
  );
}

