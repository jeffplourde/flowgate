import { useFlowgateSocket } from "./hooks/useFlowgateSocket";
import { InstancePanel } from "./components/InstancePanel";
import { ControlPanel } from "./components/ControlPanel";

function App() {
  const { instanceA, instanceB, connected, updateConfig } =
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
            gap: "8px",
            fontSize: "12px",
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

      <ControlPanel updateConfig={updateConfig} />
    </div>
  );
}

export default App;
