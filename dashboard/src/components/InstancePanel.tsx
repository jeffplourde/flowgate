import type { InstanceState } from "../types";
import { MetricGauge } from "./MetricGauge";
import { TimeSeriesChart } from "./TimeSeriesChart";
import { ScoreHistogram } from "./ScoreHistogram";

interface Props {
  title: string;
  state: InstanceState;
  color: string;
}

export function InstancePanel({ title, state, color }: Props) {
  return (
    <div
      style={{
        flex: 1,
        padding: "16px",
        background: "#111",
        borderRadius: "8px",
        minWidth: 0,
      }}
    >
      <h2
        style={{
          margin: "0 0 12px 0",
          fontSize: "16px",
          color,
          borderBottom: `2px solid ${color}`,
          paddingBottom: "8px",
        }}
      >
        {title}
      </h2>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr 1fr 1fr",
          gap: "8px",
          marginBottom: "12px",
        }}
      >
        <MetricGauge label="Avg Score" value={state.avgScore} precision={3} />
        <MetricGauge
          label="Avg Latency"
          value={state.avgLatency}
          unit="ms"
          precision={0}
        />
        <MetricGauge label="Emitted" value={state.emitted} precision={0} />
        <MetricGauge
          label="State"
          value={state.controllerState}
          precision={0}
        />
      </div>

      <TimeSeriesChart
        title="Emission Rate"
        data={state.emissionRate}
        color={color}
        referenceLine={state.targetRate}
        referenceLabel="target"
      />

      <TimeSeriesChart
        title="Threshold"
        data={state.threshold}
        color="#e67e22"
        domain={[0, 1]}
      />

      {state.bufferSize.length > 0 &&
        state.bufferSize.some((p) => p.value > 0) && (
          <TimeSeriesChart
            title="Buffer Size"
            data={state.bufferSize}
            color="#9b59b6"
          />
        )}

      <ScoreHistogram scores={state.scoreHistory} color={color} />

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr 1fr",
          gap: "4px",
          fontSize: "11px",
          color: "#666",
          fontFamily: "monospace",
        }}
      >
        <div>P: {state.pidP.toFixed(6)}</div>
        <div>I: {state.pidI.toFixed(6)}</div>
        <div>D: {state.pidD.toFixed(6)}</div>
        <div>Received: {state.received}</div>
        <div>Rejected: {state.rejected}</div>
        <div>Evicted: {state.evicted}</div>
      </div>
    </div>
  );
}
