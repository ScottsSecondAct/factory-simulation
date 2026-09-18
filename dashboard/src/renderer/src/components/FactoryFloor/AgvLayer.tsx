import type { AgvSnap } from "../../types";
import type { FloorLayout } from "./layout";
import { segmentCenter } from "./layout";

const AGV_STATE_COLOR: Record<string, string> = {
  Idle: "#06b6d4",
  Traveling: "#06b6d4",
  Loading: "#06b6d4",
  Unloading: "#fbbf24",
  Blocked: "#eab308",
  Faulted: "#ef4444",
};

const AMR_STATE_COLOR: Record<string, string> = {
  Idle: "#a78bfa",
  Traveling: "#a78bfa",
  Loading: "#a78bfa",
  Unloading: "#fbbf24",
  Blocked: "#eab308",
  Faulted: "#ef4444",
};

interface Props {
  agvs: AgvSnap[];
  fl: FloorLayout;
  loopSegments: number;
}

export function AgvLayer({ agvs, fl, loopSegments }: Props) {
  return (
    <g>
      {agvs.map((a) => {
        const pos = segmentCenter(a.segment, fl, loopSegments);
        const isAmr = a.vehicle_type === "Amr";
        const colorMap = isAmr ? AMR_STATE_COLOR : AGV_STATE_COLOR;
        const color = colorMap[a.state] || colorMap.Idle;
        const faulted = a.state === "Faulted";
        const pathStroke = isAmr ? "rgba(167,139,250,0.3)" : "rgba(6,182,212,0.3)";

        const pathPts: { x: number; y: number }[] = [];
        if (a.state === "Traveling" && a.path.length > 0) {
          for (let i = a.path_cursor; i < a.path.length; i++) {
            pathPts.push(segmentCenter(a.path[i], fl, loopSegments));
          }
        }

        return (
          <g key={a.id}>
            {pathPts.length > 1 && (
              <polyline
                points={pathPts.map((p) => `${p.x},${p.y}`).join(" ")}
                fill="none"
                stroke={pathStroke}
                strokeWidth={1.5}
                strokeDasharray="3 4"
              />
            )}
            {isAmr ? (
              <rect
                x={pos.x - 6}
                y={pos.y - 6}
                width={12}
                height={12}
                rx={2}
                fill={color}
                stroke="#0f1117"
                strokeWidth={1.5}
                transform={`rotate(45 ${pos.x} ${pos.y})`}
                style={faulted ? { animation: "pulse-red 1s infinite" } : undefined}
              />
            ) : (
              <circle
                cx={pos.x}
                cy={pos.y}
                r={7}
                fill={color}
                stroke="#0f1117"
                strokeWidth={1.5}
                style={faulted ? { animation: "pulse-red 1s infinite" } : undefined}
              />
            )}
            <text
              x={pos.x}
              y={pos.y + 3.5}
              textAnchor="middle"
              fill="#0f1117"
              fontSize={8}
              fontWeight={700}
            >
              {isAmr ? "M" : a.id}
            </text>
          </g>
        );
      })}
    </g>
  );
}
