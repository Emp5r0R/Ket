import type { TrafficPoint } from "../types";

interface TrafficChartProps {
  points: TrafficPoint[];
  metric: "received" | "sent";
}

export function TrafficChart({ points, metric }: TrafficChartProps) {
  const samples = points.map((point) => (metric === "received" ? point.received : point.sent));
  const values = samples.length === 1 ? [samples[0], samples[0]] : samples;
  const width = 520;
  const height = 150;
  const padding = 8;
  const min = values.length > 0 ? Math.min(...values) : 0;
  const max = values.length > 0 ? Math.max(...values) : 0;
  const spread = Math.max(max - min, 1);
  const coordinates = values.map((value, index) => {
    const x =
      values.length <= 1
        ? padding
        : padding + (index / (values.length - 1)) * (width - padding * 2);
    const y =
      max === min
        ? height / 2
        : height - padding - ((value - min) / spread) * (height - padding * 2);
    return [x, y] as const;
  });
  const line = coordinates.map(([x, y], index) => `${index === 0 ? "M" : "L"}${x},${y}`).join(" ");
  const area = coordinates.length
    ? `${line} L${coordinates.at(-1)?.[0]},${height} L${coordinates[0][0]},${height} Z`
    : "";

  return (
    <svg
      className={`traffic-chart chart-${metric}`}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`${metric === "received" ? "Received" : "Sent"} traffic history`}
    >
      <line className="chart-grid" x1="0" x2={width} y1="38" y2="38" />
      <line className="chart-grid" x1="0" x2={width} y1="76" y2="76" />
      <line className="chart-grid" x1="0" x2={width} y1="114" y2="114" />
      {area ? <path className="chart-area" d={area} /> : null}
      {line ? <path className="chart-line" d={line} /> : null}
    </svg>
  );
}
