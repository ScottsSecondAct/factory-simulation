import { useRef, useEffect, useState } from "react";
import { useStore } from "../store";

const KIND_COLOR: Record<string, string> = {
  fault: "var(--red)",
  repair: "var(--green)",
  deadlock: "var(--orange)",
  dispatch: "var(--cyan)",
  completion: "var(--text-dim)",
  log: "var(--text-dim)",
  queue_alert: "var(--orange)",
};

function formatTime(t: number): string {
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = Math.floor(t % 60);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export function EventLog() {
  const events = useStore((s) => s.events);
  const listRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);

  useEffect(() => {
    if (autoScroll && listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight;
    }
  }, [events, autoScroll]);

  const handleScroll = () => {
    if (!listRef.current) return;
    const el = listRef.current;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    setAutoScroll(atBottom);
  };

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <h2 style={styles.heading}>Event Log</h2>
        <span style={styles.count} className="tabular-nums">
          {events.length}
        </span>
      </div>
      <div ref={listRef} style={styles.list} onScroll={handleScroll}>
        {events.map((e, i) => (
          <div key={i} style={styles.entry}>
            <span style={styles.time} className="tabular-nums">
              [{formatTime(e.time)}]
            </span>
            <span style={{ color: KIND_COLOR[e.kind] || "var(--text-dim)" }}>
              {e.text}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    flexDirection: "column" as const,
    borderTop: "1px solid var(--border)",
    height: 160,
    flexShrink: 0,
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
    padding: "6px 12px",
  },
  heading: {
    fontSize: 11,
    fontWeight: 600,
    textTransform: "uppercase" as const,
    letterSpacing: "0.06em",
    color: "var(--text-dim)",
  },
  count: { fontSize: 11, color: "var(--text-dim)" },
  list: {
    flex: 1,
    overflowY: "auto" as const,
    padding: "0 12px 8px",
    fontFamily: "'SF Mono', 'Cascadia Code', Consolas, monospace",
    fontSize: 12,
  },
  entry: {
    padding: "1px 0",
    lineHeight: 1.5,
  },
  time: {
    color: "var(--text-dim)",
    marginRight: 6,
  },
};
