import { ArrowDown, ArrowUp, Gauge, Radio } from "lucide-react";
import { formatBytes, formatLatency } from "../lib/format";
import type { ClientSnapshot } from "../types";

interface StatStripProps {
  snapshot: ClientSnapshot;
}

export function StatStrip({ snapshot }: StatStripProps) {
  const node = snapshot.node;
  const traffic = snapshot.traffic;
  const stats = [
    {
      label: "Received",
      value: traffic?.available ? formatBytes(traffic.bytes_received) : "--",
      icon: ArrowDown,
      tone: "green",
    },
    {
      label: "Sent",
      value: traffic?.available ? formatBytes(traffic.bytes_sent) : "--",
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
      {stats.map(({ label, value, icon: Icon, tone }) => (
        <div className="stat-item" key={label}>
          <span className={`stat-icon tone-${tone}`}>
            <Icon size={18} aria-hidden="true" />
          </span>
          <span className="stat-copy">
            <span>{label}</span>
            <strong>{value}</strong>
          </span>
        </div>
      ))}
    </div>
  );
}

