import { peakRate, type RatePoint } from "../lib/throughput";

interface TrafficChartProps {
  rates: RatePoint[];
  metric: "received" | "sent";
}

const WIDTH = 520;
const HEIGHT = 150;
const PADDING = 8;

/** Speed over time, scaled from zero to the highest speed shown. */
export function TrafficChart({ rates, metric }: TrafficChartProps) {
  const values = rates.map((rate) => rate[metric]);
  const series = values.length === 1 ? [values[0], values[0]] : values;
  const top = Math.max(peakRate(rates, metric), 1);
  const coordinates = series.map((value, index) => {
    const x = series.length <= 1
      ? PADDING
      : PADDING + (index / (series.length - 1)) * (WIDTH - PADDING * 2);
    const y = HEIGHT - PADDING - (value / top) * (HEIGHT - PADDING * 2);
    return [x, y] as const;
  });
  const line = coordinates.map(([x, y], index) => `${index === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = coordinates.length
    ? `${line} L${coordinates.at(-1)?.[0].toFixed(1)},${HEIGHT} L${coordinates[0][0].toFixed(1)},${HEIGHT} Z`
    : "";

  return (
    <svg
      className={`traffic-chart chart-${metric}`}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={`${metric === "received" ? "Download" : "Upload"} speed history`}
    >
      <line className="chart-grid" x1="0" x2={WIDTH} y1="38" y2="38" />
      <line className="chart-grid" x1="0" x2={WIDTH} y1="76" y2="76" />
      <line className="chart-grid" x1="0" x2={WIDTH} y1="114" y2="114" />
      {area ? <path className="chart-area" d={area} /> : null}
      {line ? <path className="chart-line" d={line} /> : null}
    </svg>
  );
}
