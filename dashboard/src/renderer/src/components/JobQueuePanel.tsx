import { useStore } from "../store";
import type { Priority } from "../types";

const PRIORITY_COLOR: Record<Priority, string> = {
  Critical: "var(--red)",
  High: "var(--orange)",
  Normal: "var(--text)",
  Low: "var(--text-dim)",
};

export function JobQueuePanel() {
  const snapshot = useStore((s) => s.snapshot);
  if (!snapshot) return null;

  const q = snapshot.job_queue;
  const alertThreshold = 20;

  return (
    <div style={styles.section}>
      <h2
        style={{
          ...styles.heading,
          color: q.depth > alertThreshold ? "var(--orange)" : "var(--text-dim)",
        }}
      >
        Job Queue ({q.depth})
      </h2>
      <div style={styles.list}>
        {q.next_8.map((j) => (
          <div key={j.id} style={styles.row}>
            <span
              style={{ ...styles.jobId, color: PRIORITY_COLOR[j.priority] }}
              className="tabular-nums"
            >
              #{j.id}
            </span>
            <span
              style={{
                ...styles.priority,
                color: PRIORITY_COLOR[j.priority],
              }}
            >
              {j.priority.slice(0, 4).toUpperCase()}
            </span>
            <span style={styles.ops}>{j.ops} op{j.ops > 1 ? "s" : ""}</span>
            <span style={styles.wait} className="tabular-nums">
              {Math.floor(j.wait_time)}s
            </span>
          </div>
        ))}
        {q.next_8.length === 0 && (
          <div style={{ fontSize: 12, color: "var(--text-dim)" }}>Empty</div>
        )}
      </div>
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
    marginBottom: 8,
  },
  list: { display: "flex", flexDirection: "column" as const, gap: 3 },
  row: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    fontSize: 12,
    padding: "2px 0",
  },
  jobId: { fontWeight: 600, width: 44 },
  priority: { fontSize: 10, fontWeight: 600, width: 38 },
  ops: { color: "var(--text-dim)", flex: 1 },
  wait: { color: "var(--text-dim)", fontSize: 11 },
};
