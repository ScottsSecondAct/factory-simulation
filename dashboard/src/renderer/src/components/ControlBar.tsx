import { useStore } from "../store";
import type { StrategyName } from "../types";

const SPEEDS = [1, 4, 16, 64, 256];

const STRATEGY_LABELS: Record<StrategyName, string> = {
  Fifo: "FIFO",
  ShortestProcessingTime: "SPT",
  EarliestDueDate: "EDD",
  WeightedPriority: "Weighted",
};

export function ControlBar() {
  const { status, config, snapshot, speedMultiplier, setSpeed, theme, toggleTheme, abActive, setStrategy, startAb, stopAb } = useStore();

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

  const strategies = config?.available_strategies ?? [];
  const currentStrategy = snapshot?.metrics.strategy ?? config?.strategy ?? "Fifo";

  const handleStrategyChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    setStrategy(e.target.value as StrategyName);
  };

  const handleAbToggle = () => {
    if (abActive) {
      stopAb();
    } else {
      const other = strategies.find((s) => s !== currentStrategy) ?? "ShortestProcessingTime";
      startAb(other as StrategyName);
    }
  };

  const handleAbStrategyChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    stopAb();
    startAb(e.target.value as StrategyName);
  };

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

      <div style={styles.center}>
        <div style={styles.clock} className="tabular-nums">
          {snapshot ? formatClock(snapshot.time) : "00:00:00"}
        </div>
        {canControl && strategies.length > 0 && (
          <div style={styles.strategyGroup}>
            <select
              style={styles.select}
              value={currentStrategy}
              onChange={handleStrategyChange}
              title="Scheduling strategy"
            >
              {strategies.map((s) => (
                <option key={s} value={s}>{STRATEGY_LABELS[s] ?? s}</option>
              ))}
            </select>
            <button
              style={{
                ...styles.btnSmall,
                ...(abActive ? { borderColor: "var(--accent)", color: "var(--accent)" } : {}),
              }}
              onClick={handleAbToggle}
              title={abActive ? "Stop A/B comparison" : "Start A/B comparison"}
            >
              A/B
            </button>
            {abActive && (
              <select
                style={styles.select}
                value={snapshot?.metrics.ab_metrics?.strategy ?? "ShortestProcessingTime"}
                onChange={handleAbStrategyChange}
                title="B strategy"
              >
                {strategies
                  .filter((s) => s !== currentStrategy)
                  .map((s) => (
                    <option key={s} value={s}>{STRATEGY_LABELS[s] ?? s}</option>
                  ))}
              </select>
            )}
          </div>
        )}
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
  center: { display: "flex", gap: 10, alignItems: "center" },
  right: { display: "flex", gap: 8, alignItems: "center" },
  clock: {
    fontSize: 20,
    fontWeight: 700,
    fontFamily: "'SF Mono', 'Cascadia Code', Consolas, monospace",
    letterSpacing: "0.02em",
  },
  strategyGroup: {
    display: "flex",
    gap: 4,
    alignItems: "center",
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
  select: {
    background: "var(--bg-card)",
    border: "1px solid var(--border)",
    color: "var(--text)",
    padding: "3px 8px",
    borderRadius: 4,
    fontSize: 12,
    cursor: "pointer",
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
