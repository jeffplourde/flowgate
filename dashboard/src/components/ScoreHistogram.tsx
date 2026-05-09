import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";

interface Props {
  scores: number[];
  color?: string;
}

function buildHistogram(scores: number[], bins: number = 20) {
  const counts = new Array(bins).fill(0);
  for (const s of scores) {
    const idx = Math.min(Math.floor(s * bins), bins - 1);
    counts[idx]++;
  }
  return counts.map((count, i) => ({
    range: (i / bins).toFixed(2),
    count,
  }));
}

export function ScoreHistogram({ scores, color = "#82ca9d" }: Props) {
  const data = buildHistogram(scores);

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
        Emitted Score Distribution
      </div>
      <ResponsiveContainer width="100%" height={100}>
        <BarChart data={data}>
          <CartesianGrid strokeDasharray="3 3" stroke="#333" />
          <XAxis dataKey="range" tick={{ fontSize: 8 }} interval={3} />
          <YAxis width={30} tick={{ fontSize: 10 }} />
          <Tooltip
            contentStyle={{ background: "#1a1a1a", border: "1px solid #333" }}
          />
          <Bar dataKey="count" fill={color} isAnimationActive={false} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
}
