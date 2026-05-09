import { useState, useCallback, useEffect } from "react";

interface ConfigEntry {
  key: string;
  value: string;
}

interface Props {
  updateConfig: (instance: string, key: string, value: string) => Promise<void>;
}

export function ProducerPanel({ updateConfig }: Props) {
  const [numClients, setNumClients] = useState(70);
  const [timeCompression, setTimeCompression] = useState(60);
  const [minBatch, setMinBatch] = useState(100);
  const [maxBatch, setMaxBatch] = useState(5000);
  const [distribution, setDistribution] = useState("beta");
  const [variance, setVariance] = useState(0.3);

  useEffect(() => {
    fetch("/api/config/producer")
      .then((r) => r.json())
      .then((entries: ConfigEntry[]) => {
        const m = Object.fromEntries(entries.map((e) => [e.key, e.value]));
        if (m.num_clients) setNumClients(parseInt(m.num_clients));
        if (m.time_compression) setTimeCompression(parseFloat(m.time_compression));
        if (m.min_batch_size) setMinBatch(parseInt(m.min_batch_size));
        if (m.max_batch_size) setMaxBatch(parseInt(m.max_batch_size));
        if (m.distribution) setDistribution(m.distribution);
        if (m.distribution_variance) setVariance(parseFloat(m.distribution_variance));
      })
      .catch(() => {});
  }, []);

  const update = useCallback(
    (key: string, value: string) => updateConfig("producer", key, value),
    [updateConfig]
  );

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr 1fr",
        gap: "16px",
        padding: "16px",
        background: "#1a1a1a",
        borderRadius: "8px",
      }}
    >
      <div style={{ gridColumn: "1 / -1" }}>
        <h3
          style={{
            margin: "0 0 8px 0",
            fontSize: "14px",
            color: "#e67e22",
          }}
        >
          Producer — Multi-Client Batch Simulator
        </h3>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Clients: {numClients}
        </label>
        <input
          type="range"
          min={1}
          max={200}
          value={numClients}
          onChange={(e) => {
            const v = parseInt(e.target.value);
            setNumClients(v);
            update("num_clients", v.toString());
          }}
          style={{ width: "100%" }}
        />
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Time Compression: {timeCompression}x
        </label>
        <input
          type="range"
          min={1}
          max={360}
          value={timeCompression}
          onChange={(e) => {
            const v = parseInt(e.target.value);
            setTimeCompression(v);
            update("time_compression", v.toString());
          }}
          style={{ width: "100%" }}
        />
        <div style={{ fontSize: "10px", color: "#555" }}>
          1 hour = {(3600 / timeCompression).toFixed(1)}s
        </div>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>Distribution</label>
        <select
          value={distribution}
          onChange={(e) => {
            setDistribution(e.target.value);
            update("distribution", e.target.value);
          }}
          style={{
            width: "100%",
            padding: "4px",
            background: "#2a2a2a",
            color: "#fff",
            border: "1px solid #444",
          }}
        >
          <option value="beta">Beta(2,5)</option>
          <option value="normal">Normal(0.5, 0.15)</option>
          <option value="uniform">Uniform(0,1)</option>
          <option value="bimodal">Bimodal</option>
        </select>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Batch Size: {minBatch} - {maxBatch}
        </label>
        <div style={{ display: "flex", gap: "8px" }}>
          <input
            type="number"
            value={minBatch}
            onChange={(e) => {
              const v = parseInt(e.target.value) || 1;
              setMinBatch(v);
              update("min_batch_size", v.toString());
            }}
            style={{
              width: "50%",
              padding: "4px",
              background: "#2a2a2a",
              color: "#fff",
              border: "1px solid #444",
              fontFamily: "monospace",
            }}
          />
          <input
            type="number"
            value={maxBatch}
            onChange={(e) => {
              const v = parseInt(e.target.value) || 1;
              setMaxBatch(v);
              update("max_batch_size", v.toString());
            }}
            style={{
              width: "50%",
              padding: "4px",
              background: "#2a2a2a",
              color: "#fff",
              border: "1px solid #444",
              fontFamily: "monospace",
            }}
          />
        </div>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Per-Client Variance: {variance.toFixed(2)}
        </label>
        <input
          type="range"
          min={0}
          max={100}
          value={variance * 100}
          onChange={(e) => {
            const v = parseInt(e.target.value) / 100;
            setVariance(v);
            update("distribution_variance", v.toString());
          }}
          style={{ width: "100%" }}
        />
      </div>
    </div>
  );
}
