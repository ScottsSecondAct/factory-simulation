import { useEffect } from "react";
import { useStore } from "./store";
import type { SimMessage } from "./types";
import { ControlBar } from "./components/ControlBar";
import { FactoryFloor } from "./components/FactoryFloor";
import { MetricsPanel } from "./components/MetricsPanel";
import { AbComparisonPanel } from "./components/AbComparisonPanel";
import { ResourcePanel } from "./components/ResourcePanel";
import { JobQueuePanel } from "./components/JobQueuePanel";
import { TrendChart } from "./components/TrendChart";
import { EventLog } from "./components/EventLog";

function eventText(msg: SimMessage & { type: "event" }): string {
  const d = msg.detail;
  switch (msg.kind) {
    case "fault": {
      const tgt = d.target as Record<string, number> | undefined;
      const label = tgt
        ? "Mill" in tgt
          ? `Mill ${tgt.Mill}`
          : `AGV ${tgt.Agv}`
        : "unknown";
      return `FAULT: ${label} down`;
    }
    case "repair": {
      const tgt = d.target as Record<string, number> | undefined;
      const label = tgt
        ? "Mill" in tgt
          ? `Mill ${tgt.Mill}`
          : `AGV ${tgt.Agv}`
        : "unknown";
      return `REPAIR: ${label} back online`;
    }
    case "deadlock":
      return `DEADLOCK: AGVs [${(d.cycle as number[])?.join(", ")}] — retreated AGV ${d.victim}`;
    case "dispatch":
      return `Dispatched Job ${d.job_id} → Mill ${d.mill_id} via AGV ${d.agv_id}`;
    case "completion":
      return `Completed Job ${d.job_id} on Mill ${d.mill_id}`;
    case "queue_alert":
      return `Queue depth alert: ${d.depth} jobs (oldest: ${d.oldest_wait}s)`;
    default:
      return JSON.stringify(d);
  }
}

export default function App() {
  const status = useStore((s) => s.status);
  const layout = useStore((s) => s.layout);
  const theme = useStore((s) => s.theme);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    const store = useStore.getState();

    const unsubs = [
      window.simBridge.onMessage((msg) => {
        const m = msg as SimMessage;
        const s = useStore.getState();

        switch (m.type) {
          case "ready":
            s.setReady(m.config, m.layout);
            break;
          case "snapshot":
            if (s.status === "ready" || s.status === "disconnected") {
              s.setStatus("running");
            }
            s.updateSnapshot(m);
            break;
          case "event":
            s.pushEvent({
              time: m.time,
              kind: m.kind,
              text: eventText(m as SimMessage & { type: "event" }),
            });
            break;
          case "summary":
            s.setSummary(m);
            break;
        }
      }),
      window.simBridge.onStderr((line) => {
        const snap = useStore.getState().snapshot;
        useStore.getState().pushLog(line, snap?.time ?? 0);
      }),
      window.simBridge.onExit(({ code, lastStderr }) => {
        if (useStore.getState().status !== "finished") {
          useStore.getState().setError(
            `Sim exited (code ${code}): ${lastStderr.slice(-3).join(" | ")}`
          );
        }
      }),
      window.simBridge.onError((msg) => {
        useStore.getState().setError(msg);
      }),
    ];

    return () => unsubs.forEach((u) => u());
  }, []);

  return (
    <div style={styles.app}>
      <ControlBar />
      <div style={styles.main}>
        <div style={styles.left}>
          {layout ? (
            <FactoryFloor layout={layout} />
          ) : (
            <div style={styles.placeholder}>
              {status === "error"
                ? useStore.getState().errorMsg
                : "Press Run to start the simulation"}
            </div>
          )}
          <TrendChart />
        </div>
        <aside style={styles.sidebar}>
          <div style={styles.sidebarScroll}>
            <MetricsPanel />
            <AbComparisonPanel />
            <ResourcePanel />
            <JobQueuePanel />
          </div>
        </aside>
      </div>
      <EventLog />
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  app: {
    display: "flex",
    flexDirection: "column" as const,
    height: "100vh",
    overflow: "hidden",
  },
  main: {
    display: "flex",
    flex: 1,
    overflow: "hidden",
  },
  left: {
    flex: 1,
    display: "flex",
    flexDirection: "column" as const,
    overflow: "hidden",
    borderRight: "1px solid var(--border)",
  },
  sidebar: {
    width: 300,
    display: "flex",
    flexDirection: "column" as const,
    overflow: "hidden",
  },
  sidebarScroll: {
    flex: 1,
    overflowY: "auto" as const,
  },
  placeholder: {
    flex: 1,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "var(--text-dim)",
    fontSize: 14,
  },
};
