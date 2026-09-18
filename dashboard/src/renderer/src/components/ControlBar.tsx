import { useStore } from "../store";

const SPEEDS = [1, 4, 16, 64, 256];

export function ControlBar() {
  const { status, config, snapshot, speedMultiplier, setSpeed, theme, toggleTheme } = useStore();

  const handleStart = async () => {
    const { setStatus, reset } = useStore.getState();
    reset();
    const result = await window.simBridge.start({ duration: 28800 });
    if (result.error) {
      useStore.getState().setError(result.error);
    }
  };

  const sendCmd = (type: string, extra?: Record<string, unknown>) => {
    window.simBridge.command({ type, ...extra });
  };

  const handleSpeed = () => {
    const idx = SPEEDS.indexOf(speedMultiplier);
    const next = SPEEDS[(idx + 1) % SPEEDS.length];
    setSpeed(next);
    sendCmd("speed", { multiplier: next });
  };

  const handleFaultMill = () => {
    const snap = useStore.getState().snapshot;
    if (!snap) return;
    const candidates = snap.mills.filter((m) => m.state !== "Faulted");
    if (candidates.length === 0) return;
    const target = candidates[Math.floor(Math.random() * candidates.length)];
    sendCmd("inject_fault", { target: { Mill: target.id } });
  };

  const handleFaultAgv = () => {
    const snap = useStore.getState().snapshot;
    if (!snap) return;
    const candidates = snap.agvs.filter((a) => a.state !== "Faulted");
    if (candidates.length === 0) return;
    const target = candidates[Math.floor(Math.random() * candidates.length)];
    sendCmd("inject_fault", { target: { Agv: target.id } });
  };

  const formatClock = (t: number) => {
    const h = Math.floor(t / 3600);
    const m = Math.floor((t % 3600) / 60);
    const s = Math.floor(t % 60);
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  };

  const isRunning = status === "running";
  const isPaused = status === "paused";
  const canControl = isRunning || isPaused;

  return (
    <header style={styles.bar}>
      <div style={styles.left}>
        {status === "disconnected" || status === "error" || status === "finished" ? (
          <button style={styles.btn} onClick={handleStart}>
            &#9654; Run
          </button>
        ) : null}
        {status === "ready" && (
          <button style={styles.btn} onClick={() => sendCmd("start")}>
            &#9654; Start
          </button>
        )}
        {isRunning && (
          <button style={styles.btn} onClick={() => { sendCmd("pause"); useStore.getState().setStatus("paused"); }}>
            &#9646;&#9646; Pause
          </button>
        )}
        {isPaused && (
          <button style={styles.btn} onClick={() => { sendCmd("resume"); useStore.getState().setStatus("running"); }}>
            &#9654; Resume
          </button>
        )}
        {canControl && (
          <button style={styles.btn} onClick={() => sendCmd("stop")}>
            &#9632; Stop
          </button>
        )}
        {canControl && (
          <button style={styles.btnSmall} onClick={handleSpeed}>
            {speedMultiplier}x
          </button>
        )}
      </div>

      <div style={styles.clock} className="tabular-nums">
        {snapshot ? formatClock(snapshot.time) : "00:00:00"}
      </div>

      <div style={styles.right}>
        {canControl && (
          <>
            <button style={styles.btnSmall} onClick={handleFaultMill}>
              Fault Mill
            </button>
            <button style={styles.btnSmall} onClick={handleFaultAgv}>
              Fault AGV
            </button>
          </>
        )}
        {config && (
          <span style={styles.configLabel}>
            {config.duration / 3600}h &middot; seed {config.seed} &middot;{" "}
            {config.faults_enabled ? "faults on" : "no faults"}
          </span>
        )}
        {status === "error" && (
          <span style={{ ...styles.configLabel, color: "var(--red)" }}>
            {useStore.getState().errorMsg}
          </span>
        )}
        <button
          style={styles.themeBtn}
          onClick={toggleTheme}
          title={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
        >
          {theme === "dark" ? "☀" : "☾"}
        </button>
      </div>
    </header>
  );
}

const styles: Record<string, React.CSSProperties> = {
  bar: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "8px 16px",
    borderBottom: "1px solid var(--border)",
    gap: 12,
    flexShrink: 0,
  },
  left: { display: "flex", gap: 6, alignItems: "center" },
  right: { display: "flex", gap: 8, alignItems: "center" },
  clock: {
    fontSize: 20,
    fontWeight: 700,
    fontFamily: "'SF Mono', 'Cascadia Code', Consolas, monospace",
    letterSpacing: "0.02em",
  },
  btn: {
    background: "none",
    border: "1px solid var(--border)",
    color: "var(--text)",
    padding: "4px 14px",
    borderRadius: 4,
    cursor: "pointer",
    fontSize: 13,
  },
  btnSmall: {
    background: "none",
    border: "1px solid var(--border)",
    color: "var(--text-dim)",
    padding: "3px 10px",
    borderRadius: 4,
    cursor: "pointer",
    fontSize: 12,
  },
  configLabel: {
    fontSize: 12,
    color: "var(--text-dim)",
  },
  themeBtn: {
    background: "none",
    border: "1px solid var(--border)",
    color: "var(--text)",
    padding: "3px 8px",
    borderRadius: 4,
    cursor: "pointer",
    fontSize: 14,
    lineHeight: 1,
  },
};
