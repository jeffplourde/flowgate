import { useState, useCallback, useEffect } from "react";

interface ConfigEntry {
  key: string;
  value: string;
}

interface Props {
  updateConfig: (instance: string, key: string, value: string) => Promise<void>;
}

export function ControlPanel({ updateConfig }: Props) {
  const [targetRate, setTargetRate] = useState(10);
  const [algoA, setAlgoA] = useState("pid");
  const [algoB, setAlgoB] = useState("buffered_streaming");
  const [bufferDuration, setBufferDuration] = useState(5000);
  const [kp, setKp] = useState("0.0004");
  const [ki, setKi] = useState("0.00004");
  const [kd, setKd] = useState("0.00001");

  useEffect(() => {
    const load = async (instance: string) => {
      try {
        const resp = await fetch(`/api/config/${instance}`);
        const entries: ConfigEntry[] = await resp.json();
        const map = Object.fromEntries(entries.map((e) => [e.key, e.value]));
        return map;
      } catch {
        return {};
      }
    };

    Promise.all([load("a"), load("b")]).then(([a, b]) => {
      if (a.target_rate) setTargetRate(parseFloat(a.target_rate));
      if (a.algorithm) setAlgoA(a.algorithm);
      if (b.algorithm) setAlgoB(b.algorithm);
      if (b.max_buffer_duration_ms)
        setBufferDuration(parseInt(b.max_buffer_duration_ms));
      if (a.kp) setKp(a.kp);
      if (a.ki) setKi(a.ki);
      if (a.kd) setKd(a.kd);
    });
  }, []);

  const applyBoth = useCallback(
    async (key: string, value: string) => {
      await Promise.all([
        updateConfig("a", key, value),
        updateConfig("b", key, value),
      ]);
    },
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
      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Target Rate (both): {targetRate}/s
        </label>
        <input
          type="range"
          min={1}
          max={100}
          value={targetRate}
          onChange={(e) => {
            const v = parseInt(e.target.value);
            setTargetRate(v);
            applyBoth("target_rate", v.toString());
          }}
          style={{ width: "100%" }}
        />
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>Algorithm A</label>
        <select
          value={algoA}
          onChange={(e) => {
            setAlgoA(e.target.value);
            updateConfig("a", "algorithm", e.target.value);
          }}
          style={{ width: "100%", padding: "4px", background: "#2a2a2a", color: "#fff", border: "1px solid #444" }}
        >
          <option value="pid">PID</option>
          <option value="fixed">Fixed</option>
          <option value="buffered_batch">Buffered Batch</option>
          <option value="buffered_streaming">Buffered Streaming</option>
        </select>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>Algorithm B</label>
        <select
          value={algoB}
          onChange={(e) => {
            setAlgoB(e.target.value);
            updateConfig("b", "algorithm", e.target.value);
          }}
          style={{ width: "100%", padding: "4px", background: "#2a2a2a", color: "#fff", border: "1px solid #444" }}
        >
          <option value="pid">PID</option>
          <option value="fixed">Fixed</option>
          <option value="buffered_batch">Buffered Batch</option>
          <option value="buffered_streaming">Buffered Streaming</option>
        </select>
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>
          Buffer Duration: {bufferDuration}ms
        </label>
        <input
          type="range"
          min={500}
          max={30000}
          step={500}
          value={bufferDuration}
          onChange={(e) => {
            const v = parseInt(e.target.value);
            setBufferDuration(v);
            updateConfig("b", "max_buffer_duration_ms", v.toString());
          }}
          style={{ width: "100%" }}
        />
      </div>

      <div>
        <label style={{ fontSize: "12px", color: "#888" }}>PID Gains (both)</label>
        <div style={{ display: "flex", gap: "4px" }}>
          {[
            { label: "Kp", val: kp, set: setKp, key: "kp" },
            { label: "Ki", val: ki, set: setKi, key: "ki" },
            { label: "Kd", val: kd, set: setKd, key: "kd" },
          ].map(({ label, val, set, key }) => (
            <input
              key={key}
              type="text"
              value={val}
              title={label}
              placeholder={label}
              onChange={(e) => set(e.target.value)}
              onBlur={() => applyBoth(key, val)}
              style={{
                width: "33%",
                padding: "4px",
                background: "#2a2a2a",
                color: "#fff",
                border: "1px solid #444",
                fontSize: "11px",
                fontFamily: "monospace",
              }}
            />
          ))}
        </div>
      </div>

      <div style={{ display: "flex", gap: "8px", alignItems: "end" }}>
        <button
          onClick={() => {
            updateConfig("a", "target_rate", "50");
            updateConfig("b", "target_rate", "50");
            setTargetRate(50);
            setTimeout(() => {
              updateConfig("a", "target_rate", "10");
              updateConfig("b", "target_rate", "10");
              setTargetRate(10);
            }, 5000);
          }}
          style={{
            padding: "8px 16px",
            background: "#c0392b",
            color: "#fff",
            border: "none",
            borderRadius: "4px",
            cursor: "pointer",
          }}
        >
          Burst (5s)
        </button>
      </div>
    </div>
  );
}
