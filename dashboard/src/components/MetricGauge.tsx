interface Props {
  label: string;
  value: number | string;
  unit?: string;
  precision?: number;
}

export function MetricGauge({ label, value, unit, precision = 2 }: Props) {
  const display =
    typeof value === "number" ? value.toFixed(precision) : value;

  return (
    <div style={{ textAlign: "center", padding: "8px" }}>
      <div style={{ fontSize: "12px", color: "#888", marginBottom: "4px" }}>
        {label}
      </div>
      <div style={{ fontSize: "24px", fontWeight: "bold", fontFamily: "monospace" }}>
        {display}
        {unit && (
          <span style={{ fontSize: "12px", color: "#888", marginLeft: "4px" }}>
            {unit}
          </span>
        )}
      </div>
    </div>
  );
}
