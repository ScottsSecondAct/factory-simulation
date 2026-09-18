import { useStore } from "../store";

export function MetricsPanel() {
  const snapshot = useStore((s) => s.snapshot);
  if (!snapshot) return null;

  const m = snapshot.metrics;
  const active = snapshot.mills.filter(
    (x) => x.state === "Machining" || x.state === "Loading" || x.state === "Unloading"
  ).length;
  const idleAgvs = snapshot.agvs.filter((a) => a.state === "Idle").length;
  const utilPct = (m.avg_utilization * 100).toFixed(1);
  const utilColor =
    m.avg_utilization > 0.7
      ? "var(--green)"
      : m.avg_utilization > 0.4
        ? "var(--yellow)"
        : "var(--red)";

  return (
    <div style={styles.section}>
      <h2 style={styles.heading}>Metrics</h2>
      <div style={styles.grid}>
        <MetricRow label="Throughput" value={String(m.throughput)} />
        <MetricRow label="Rate" value={`${m.throughput_rate.toFixed(1)}/hr`} />
        <div style={styles.row}>
          <span style={styles.label}>Utilization</span>
          <div style={styles.barTrack}>
            <div
              style={{
                ...styles.barFill,
                width: `${Math.min(m.avg_utilization * 100, 100)}%`,
                background: utilColor,
              }}
            />
          </div>
          <span style={{ ...styles.value, color: utilColor }}>{utilPct}%</span>
        </div>
        <MetricRow label="WIP" value={`${m.wip}/${m.max_wip}`} />
        <MetricRow label="Back-pressure" value={String(m.back_pressure_events)} />
        <MetricRow label="Chip evacs" value={String(m.chip_evacuations)} />
        <MetricRow label="Work prep" value={`${m.work_prep_jobs} (Q:${m.work_prep_queue})`} />
        <MetricRow
          label="Reconciliation"
          value={`${m.reconciliation_passes} passes, ${m.reconciliation_drifts} drifts`}
        />
        <MetricRow label="Queue" value={String(snapshot.job_queue.depth)} />
        <MetricRow label="Deadlocks" value={String(m.deadlocks)} />
        <MetricRow label="Faults" value={String(m.faults)} />
        <MetricRow label="Active mills" value={`${active}/25`} />
        <MetricRow label="Idle AGVs" value={`${idleAgvs}/6`} />
        <MetricRow label="Events" value={snapshot.event_count.toLocaleString()} />
      </div>
    </div>
  );
}

function MetricRow({ label, value }: { label: string; value: string }) {
  return (
    <div style={styles.row}>
      <span style={styles.label}>{label}</span>
      <span style={styles.value} className="tabular-nums">{value}</span>
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
  grid: { display: "flex", flexDirection: "column" as const, gap: 5 },
  row: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    fontSize: 13,
  },
  label: { color: "var(--text-dim)", fontSize: 12 },
  value: { fontWeight: 600, fontSize: 13 },
  barTrack: {
    flex: 1,
    height: 4,
    background: "var(--border)",
    borderRadius: 2,
    margin: "0 8px",
    overflow: "hidden" as const,
  },
  barFill: {
    height: "100%",
    borderRadius: 2,
    transition: "width 0.3s",
  },
};
