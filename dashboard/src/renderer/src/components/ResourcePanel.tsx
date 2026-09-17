import { useStore } from "../store";

export function ResourcePanel() {
  const snapshot = useStore((s) => s.snapshot);
  const config = useStore((s) => s.config);
  if (!snapshot || !config) return null;

  const tc = snapshot.tool_crib;
  const pm = snapshot.pallet_magazine;
  const totalTools = config.tool_types * 4;
  const totalPallets = config.pallet_types * 8;
  const availTools = Object.values(tc.inventory).reduce((a, b) => a + b, 0);
  const availPallets = Object.values(pm.available).reduce((a, b) => a + b, 0);

  return (
    <div style={styles.section}>
      <h2 style={styles.heading}>Resources</h2>

      <div style={styles.group}>
        <div style={styles.groupHeader}>
          <span>Tool Crib</span>
          <span className="tabular-nums" style={{ fontWeight: 600 }}>
            {availTools}/{totalTools}
          </span>
        </div>
        <div style={styles.typeRow}>
          {Array.from({ length: config.tool_types }, (_, i) => {
            const avail = tc.inventory[String(i)] ?? 0;
            const empty = avail === 0;
            return (
              <span
                key={i}
                style={{
                  ...styles.badge,
                  color: empty ? "var(--red)" : "var(--text-dim)",
                  borderColor: empty ? "var(--red)" : "var(--border)",
                }}
              >
                T{i}:{avail}
              </span>
            );
          })}
        </div>
      </div>

      <div style={{ ...styles.group, marginTop: 10 }}>
        <div style={styles.groupHeader}>
          <span>Pallet Magazine</span>
          <span className="tabular-nums" style={{ fontWeight: 600 }}>
            {availPallets}/{totalPallets}
          </span>
        </div>
        <div style={styles.typeRow}>
          {Array.from({ length: config.pallet_types }, (_, i) => {
            const avail = pm.available[String(i)] ?? 0;
            const empty = avail === 0;
            return (
              <span
                key={i}
                style={{
                  ...styles.badge,
                  color: empty ? "var(--red)" : "var(--text-dim)",
                  borderColor: empty ? "var(--red)" : "var(--border)",
                }}
              >
                P{i}:{avail}
              </span>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  section: { padding: "12px 14px", borderBottom: "1px solid var(--border)" },
  heading: {
    fontSize: 10,
    fontWeight: 600,
    textTransform: "uppercase" as const,
    letterSpacing: "0.06em",
    color: "var(--text-dim)",
    marginBottom: 8,
  },
  group: {},
  groupHeader: {
    display: "flex",
    justifyContent: "space-between",
    fontSize: 12,
    marginBottom: 4,
  },
  typeRow: {
    display: "flex",
    gap: 4,
    flexWrap: "wrap" as const,
  },
  badge: {
    fontSize: 10,
    padding: "1px 5px",
    border: "1px solid var(--border)",
    borderRadius: 3,
    fontFamily: "'SF Mono', Consolas, monospace",
  },
};
