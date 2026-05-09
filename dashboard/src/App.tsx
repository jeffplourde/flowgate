import { useFlowgateSocket } from "./hooks/useFlowgateSocket";
import { InstancePanel } from "./components/InstancePanel";
import { ControlPanel } from "./components/ControlPanel";
import { ProducerPanel } from "./components/ProducerPanel";

function App() {
  const { instanceA, instanceB, producer, connected, updateConfig } =
    useFlowgateSocket();

  return (
    <div
      style={{
        maxWidth: "1400px",
        margin: "0 auto",
        padding: "16px",
        fontFamily:
          '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        color: "#eee",
        background: "#0a0a0a",
        minHeight: "100vh",
      }}
    >
      {producer.backpressure && (
        <div
          style={{
            background: "#c0392b",
            color: "#fff",
            padding: "10px 16px",
            borderRadius: "6px",
            marginBottom: "12px",
            fontWeight: "bold",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <span>
            BACKPRESSURE: Producer publish calls are being rejected by NATS
            ({producer.errors.toLocaleString()} errors)
          </span>
          <span style={{ fontSize: "12px", fontWeight: "normal" }}>
            {producer.published.toLocaleString()} published /{" "}
            {producer.batches.toLocaleString()} batches
          </span>
        </div>
      )}

      <header
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "16px",
          borderBottom: "1px solid #333",
          paddingBottom: "12px",
        }}
      >
        <h1 style={{ margin: 0, fontSize: "20px" }}>
          Flowgate Demonstrator
        </h1>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: "16px",
            fontSize: "12px",
          }}
        >
          <span
            style={{
              color: producer.backpressure ? "#e74c3c" : "#888",
            }}
          >
            Producer: {producer.activeClients} clients,{" "}
            {producer.published.toLocaleString()} published
            {producer.errors > 0 && (
              <span style={{ color: "#e74c3c" }}>
                {" "}
                ({producer.errors.toLocaleString()} errors)
              </span>
            )}
          </span>
          <span
            style={{
              display: "flex",
              alignItems: "center",
              gap: "6px",
              color: connected ? "#2ecc71" : "#e74c3c",
            }}
          >
            <span
              style={{
                width: "8px",
                height: "8px",
                borderRadius: "50%",
                background: connected ? "#2ecc71" : "#e74c3c",
                display: "inline-block",
              }}
            />
            {connected ? "Connected" : "Disconnected"}
          </span>
        </div>
      </header>

      <div style={{ display: "flex", gap: "16px", marginBottom: "16px" }}>
        <InstancePanel
          title="Instance A — Threshold (PID)"
          state={instanceA}
          color="#3498db"
        />
        <InstancePanel
          title="Instance B — Buffered"
          state={instanceB}
          color="#2ecc71"
        />
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
        <ControlPanel updateConfig={updateConfig} />
        <ProducerPanel updateConfig={updateConfig} />
      </div>
    </div>
  );
}

export default App;
