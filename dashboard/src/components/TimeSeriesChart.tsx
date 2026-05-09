import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  ResponsiveContainer,
} from "recharts";
import type { TimeSeriesPoint } from "../types";

interface Props {
  title: string;
  data: TimeSeriesPoint[];
  color?: string;
  referenceLine?: number;
  referenceLabel?: string;
  domain?: [number, number];
}

export function TimeSeriesChart({
  title,
  data,
  color = "#8884d8",
  referenceLine,
  referenceLabel,
  domain,
}: Props) {
  const chartData = data.map((p) => ({
    time: new Date(p.time).toLocaleTimeString(),
    value: p.value,
  }));

  return (
    <div style={{ marginBottom: "16px" }}>
      <div
        style={{
          fontSize: "12px",
          color: "#888",
          marginBottom: "4px",
          fontWeight: "bold",
        }}
      >
        {title}
      </div>
      <ResponsiveContainer width="100%" height={120}>
        <LineChart data={chartData}>
          <CartesianGrid strokeDasharray="3 3" stroke="#333" />
          <XAxis dataKey="time" tick={false} />
          <YAxis domain={domain} width={50} tick={{ fontSize: 10 }} />
          <Tooltip
            contentStyle={{ background: "#1a1a1a", border: "1px solid #333" }}
          />
          <Line
            type="monotone"
            dataKey="value"
            stroke={color}
            dot={false}
            strokeWidth={2}
            isAnimationActive={false}
          />
          {referenceLine !== undefined && (
            <ReferenceLine
              y={referenceLine}
              stroke="#ff7300"
              strokeDasharray="5 5"
              label={{
                value: referenceLabel ?? "",
                fill: "#ff7300",
                fontSize: 10,
              }}
            />
          )}
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
