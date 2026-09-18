import { useStore } from "../store";
import type { StrategyName } from "../types";

const STRATEGY_LABELS: Record<StrategyName, string> = {
  Fifo: "FIFO",
  ShortestProcessingTime: "SPT",
  EarliestDueDate: "EDD",
  WeightedPriority: "Weighted",
};

export function AbComparisonPanel() {
  const snapshot = useStore((s) => s.snapshot);
  const abActive = useStore((s) => s.abActive);
  if (!snapshot || !abActive) return null;

  const a = snapshot.metrics;
  const b = a.ab_metrics;
  if (!b) return null;

  const aLabel = STRATEGY_LABELS[a.strategy] ?? a.strategy;
  const bLabel = STRATEGY_LABELS[b.strategy] ?? b.strategy;

  return (
    <div style={styles.section}>
      <h2 style={styles.heading}>A/B Comparison</h2>
      <div style={styles.headerRow}>
        <span style={styles.metricLabel} />
        <span style={{ ...styles.colHeader, color: "var(--accent)" }}>A: {aLabel}</span>
        <span style={{ ...styles.colHeader, color: "var(--purple)" }}>B: {bLabel}</span>
        <span style={styles.colHeader}>Delta</span>
      </div>
      <CompareRow label="Throughput" a={a.throughput} b={b.throughput} />
      <CompareRow label="Rate/hr" a={a.throughput_rate} b={b.throughput_rate} decimals={1} />
      <CompareRow label="Utilization" a={a.avg_utilization * 100} b={b.avg_utilization * 100} suffix="%" decimals={1} />
      <CompareRow label="WIP" a={a.wip} b={b.wip} />
      <CompareRow label="Deadlocks" a={a.deadlocks} b={b.deadlocks} lowerBetter />
      <CompareRow label="Back-pressure" a={a.back_pressure_events} b={b.back_pressure_events} lowerBetter />
    </div>
  );
}

function CompareRow({
  label,
  a,
  b,
  decimals = 0,
  suffix = "",
  lowerBetter = false,
}: {
  label: string;
  a: number;
  b: number;
  decimals?: number;
  suffix?: string;
  lowerBetter?: boolean;
}) {
  const delta = b - a;
  const better = lowerBetter ? delta < 0 : delta > 0;
  const worse = lowerBetter ? delta > 0 : delta < 0;
  const deltaColor = better ? "var(--green)" : worse ? "var(--red)" : "var(--text-dim)";
  const deltaSign = delta > 0 ? "+" : "";
  const fmt = (v: number) => v.toFixed(decimals) + suffix;

  return (
    <div style={styles.dataRow}>
      <span style={styles.metricLabel}>{label}</span>
      <span style={styles.cellValue} className="tabular-nums">{fmt(a)}</span>
      <span style={styles.cellValue} className="tabular-nums">{fmt(b)}</span>
      <span style={{ ...styles.cellValue, color: deltaColor }} className="tabular-nums">
        {deltaSign}{fmt(delta)}
      </span>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  section: { padding: "12px 14px", borderBottom: "1px solid var(--border)" },
  heading: {
    fontSize: 11,
    fontWeight: 600,
    textTransform: "uppercase" as const,
    letterSpacing: "0.06em",
    color: "var(--text-dim)",
    marginBottom: 8,
  },
  headerRow: {
    display: "grid",
    gridTemplateColumns: "1fr 1fr 1fr 1fr",
    gap: 4,
    marginBottom: 4,
  },
  dataRow: {
    display: "grid",
    gridTemplateColumns: "1fr 1fr 1fr 1fr",
    gap: 4,
    padding: "2px 0",
    fontSize: 12,
  },
  metricLabel: {
    fontSize: 11,
    color: "var(--text-dim)",
  },
  colHeader: {
    fontSize: 10,
    fontWeight: 600,
    textTransform: "uppercase" as const,
    letterSpacing: "0.04em",
    textAlign: "right" as const,
  },
  cellValue: {
    fontWeight: 600,
    fontSize: 12,
    textAlign: "right" as const,
  },
};
