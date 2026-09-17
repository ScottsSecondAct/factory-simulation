import type { AgvSnap } from "../../types";
import type { FloorLayout } from "./layout";
import { segmentCenter } from "./layout";

const STATE_COLOR: Record<string, string> = {
  Idle: "#06b6d4",
  Traveling: "#06b6d4",
  Loading: "#06b6d4",
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
        const color = STATE_COLOR[a.state] || STATE_COLOR.Idle;
        const faulted = a.state === "Faulted";

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
                stroke="rgba(6,182,212,0.3)"
                strokeWidth={1.5}
                strokeDasharray="3 4"
              />
            )}
            <circle
              cx={pos.x}
              cy={pos.y}
              r={7}
              fill={color}
              stroke="#0f1117"
              strokeWidth={1.5}
              style={faulted ? { animation: "pulse-red 1s infinite" } : undefined}
            />
            <text
              x={pos.x}
              y={pos.y + 3.5}
              textAnchor="middle"
              fill="#0f1117"
              fontSize={8}
              fontWeight={700}
            >
              {a.id}
            </text>
          </g>
        );
      })}
    </g>
  );
}
