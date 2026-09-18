import { useRef, useEffect } from "react";
import { useStore } from "../store";
import type { TimeSeriesPoint } from "../types";

const COLORS: Record<string, string> = {
  throughput: "#6c8cff",
  utilization: "#34d399",
  queue_depth: "#fbbf24",
  active_mills: "#22d3ee",
};

const LABELS: Record<string, string> = {
  throughput: "Throughput (parts)",
  utilization: "Avg Utilization",
  queue_depth: "Queue Depth",
  active_mills: "Active Mills",
};

export function TrendChart() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const trendMetric = useStore((s) => s.trendMetric);
  const setTrendMetric = useStore((s) => s.setTrendMetric);
  const throughputHistory = useStore((s) => s.throughputHistory);
  const utilizationHistory = useStore((s) => s.utilizationHistory);
  const queueHistory = useStore((s) => s.queueHistory);
  const activeMillsHistory = useStore((s) => s.activeMillsHistory);

  const dataMap: Record<string, TimeSeriesPoint[]> = {
    throughput: throughputHistory,
    utilization: utilizationHistory,
    queue_depth: queueHistory,
    active_mills: activeMillsHistory,
  };

  const data = dataMap[trendMetric] || [];

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * dpr;
    canvas.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const w = rect.width;
    const h = rect.height;
    ctx.clearRect(0, 0, w, h);

    if (data.length < 2) return;

    const values = data.map((p) => p.value);
    const maxVal = Math.max(...values, 1);
    const minVal = Math.min(...values, 0);
    const range = maxVal - minVal || 1;

    const color = COLORS[trendMetric] || COLORS.throughput;

    // Grid lines
    ctx.strokeStyle = "rgba(99, 109, 138, 0.2)";
    ctx.lineWidth = 0.5;
    for (let i = 0; i <= 4; i++) {
      const y = 8 + ((h - 16) * i) / 4;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(w, y);
      ctx.stroke();
    }

    // Data line
    ctx.beginPath();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    data.forEach((p, i) => {
      const x = (i / (data.length - 1)) * w;
      const y = h - 8 - ((p.value - minVal) / range) * (h - 16);
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();

    // Fill under
    const lastX = w;
    ctx.lineTo(lastX, h);
    ctx.lineTo(0, h);
    ctx.closePath();
    ctx.fillStyle = color.replace(")", ", 0.08)").replace("rgb", "rgba");
    ctx.fill();
  }, [data, trendMetric]);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <select
          value={trendMetric}
          onChange={(e) => setTrendMetric(e.target.value as typeof trendMetric)}
          style={styles.select}
        >
          {Object.entries(LABELS).map(([k, v]) => (
            <option key={k} value={k}>
              {v}
            </option>
          ))}
        </select>
      </div>
      <canvas ref={canvasRef} style={styles.canvas} />
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    display: "flex",
    flexDirection: "column" as const,
    borderTop: "1px solid var(--border)",
    padding: "8px 12px",
    height: 160,
    flexShrink: 0,
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    marginBottom: 4,
  },
  select: {
    background: "var(--bg-card)",
    color: "var(--text-dim)",
    border: "1px solid var(--border)",
    borderRadius: 3,
    fontSize: 12,
    padding: "2px 6px",
    outline: "none",
  },
  canvas: {
    flex: 1,
    width: "100%",
  },
};
