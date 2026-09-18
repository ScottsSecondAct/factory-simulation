import { create } from "zustand";
import type {
  ReadyConfig,
  FactoryLayout,
  Snapshot,
  LogEntry,
  TimeSeriesPoint,
  SummaryMessage,
  MillId,
  SimTime,
  StrategyName,
} from "./types";

type Status = "disconnected" | "ready" | "running" | "paused" | "finished" | "error";
type TrendMetric = "throughput" | "utilization" | "queue_depth" | "active_mills";
export type Theme = "dark" | "light";

function getInitialTheme(): Theme {
  try {
    const saved = localStorage.getItem("factory-sim-theme");
    if (saved === "light" || saved === "dark") return saved;
  } catch { /* noop */ }
  return "dark";
}

const MAX_HISTORY = 1800;
const MAX_LOG = 500;

interface SimState {
  status: Status;
  errorMsg: string | null;
  config: ReadyConfig | null;
  layout: FactoryLayout | null;
  snapshot: Snapshot | null;
  previousSnapshot: Snapshot | null;
  summary: SummaryMessage | null;

  throughputHistory: TimeSeriesPoint[];
  utilizationHistory: TimeSeriesPoint[];
  queueHistory: TimeSeriesPoint[];
  activeMillsHistory: TimeSeriesPoint[];

  events: LogEntry[];

  selectedMill: MillId | null;
  trendMetric: TrendMetric;
  speedMultiplier: number;
  theme: Theme;
  abActive: boolean;

  setStatus: (s: Status) => void;
  setError: (msg: string) => void;
  setReady: (config: ReadyConfig, layout: FactoryLayout) => void;
  updateSnapshot: (snap: Snapshot) => void;
  pushEvent: (entry: LogEntry) => void;
  pushLog: (line: string, time: SimTime) => void;
  setSummary: (s: SummaryMessage) => void;
  setSelectedMill: (id: MillId | null) => void;
  setTrendMetric: (m: TrendMetric) => void;
  setSpeed: (s: number) => void;
  toggleTheme: () => void;
  setStrategy: (s: StrategyName) => void;
  startAb: (s: StrategyName) => void;
  stopAb: () => void;
  reset: () => void;
}

function pushRing<T>(arr: T[], item: T, max: number): T[] {
  const next = [...arr, item];
  return next.length > max ? next.slice(next.length - max) : next;
}

export const useStore = create<SimState>((set) => ({
  status: "disconnected",
  errorMsg: null,
  config: null,
  layout: null,
  snapshot: null,
  previousSnapshot: null,
  summary: null,

  throughputHistory: [],
  utilizationHistory: [],
  queueHistory: [],
  activeMillsHistory: [],

  events: [],

  selectedMill: null,
  trendMetric: "throughput",
  speedMultiplier: 1,
  theme: getInitialTheme(),
  abActive: false,

  setStatus: (s) => set({ status: s }),
  setError: (msg) => set({ status: "error", errorMsg: msg }),
  setReady: (config, layout) =>
    set({
      status: "ready",
      config,
      layout,
      snapshot: null,
      previousSnapshot: null,
      summary: null,
      throughputHistory: [],
      utilizationHistory: [],
      queueHistory: [],
      activeMillsHistory: [],
      events: [],
      errorMsg: null,
    }),
  updateSnapshot: (snap) =>
    set((state) => {
      const active = snap.mills.filter(
        (m) =>
          m.state === "Machining" ||
          m.state === "Loading" ||
          m.state === "Unloading"
      ).length;

      return {
        snapshot: snap,
        previousSnapshot: state.snapshot,
        throughputHistory: pushRing(
          state.throughputHistory,
          { time: snap.time, value: snap.metrics.throughput },
          MAX_HISTORY
        ),
        utilizationHistory: pushRing(
          state.utilizationHistory,
          { time: snap.time, value: snap.metrics.avg_utilization },
          MAX_HISTORY
        ),
        queueHistory: pushRing(
          state.queueHistory,
          { time: snap.time, value: snap.job_queue.depth },
          MAX_HISTORY
        ),
        activeMillsHistory: pushRing(
          state.activeMillsHistory,
          { time: snap.time, value: active },
          MAX_HISTORY
        ),
      };
    }),
  pushEvent: (entry) =>
    set((state) => ({
      events: pushRing(state.events, entry, MAX_LOG),
    })),
  pushLog: (line, time) =>
    set((state) => ({
      events: pushRing(
        state.events,
        { time, kind: "log", text: line },
        MAX_LOG
      ),
    })),
  setSummary: (s) => set({ summary: s, status: "finished" }),
  setSelectedMill: (id) => set({ selectedMill: id }),
  setTrendMetric: (m) => set({ trendMetric: m }),
  setSpeed: (s) => set({ speedMultiplier: s }),
  toggleTheme: () =>
    set((state) => {
      const next = state.theme === "dark" ? "light" : "dark";
      try { localStorage.setItem("factory-sim-theme", next); } catch { /* noop */ }
      return { theme: next };
    }),
  setStrategy: (s) => {
    window.simBridge.command({ type: "set_strategy", strategy: s });
  },
  startAb: (s) => {
    window.simBridge.command({ type: "start_ab", strategy: s });
    set({ abActive: true });
  },
  stopAb: () => {
    window.simBridge.command({ type: "stop_ab" });
    set({ abActive: false });
  },
  reset: () =>
    set({
      status: "disconnected",
      errorMsg: null,
      config: null,
      layout: null,
      snapshot: null,
      previousSnapshot: null,
      summary: null,
      throughputHistory: [],
      utilizationHistory: [],
      queueHistory: [],
      activeMillsHistory: [],
      events: [],
    }),
}));
