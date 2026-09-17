import type { MillSnap, MillId } from "../../types";
import type { FloorLayout } from "./layout";
import { CELL_W, CELL_H } from "./layout";

const STATE_FILL: Record<string, string> = {
  Idle: "#2a3548",
  Machining: "#166534",
  Loading: "#1e3a5f",
  Unloading: "#1e3a5f",
  ToolChange: "#78350f",
  WaitingPallet: "#713f12",
  WaitingTool: "#713f12",
  Faulted: "#7f1d1d",
};

interface Props {
  mills: MillSnap[];
  fl: FloorLayout;
  selectedMill: MillId | null;
  onSelectMill: (id: MillId | null) => void;
}

export function MillGrid({ mills, fl, selectedMill, onSelectMill }: Props) {
  const rw = CELL_W * 0.8;
  const rh = CELL_H * 0.8;

  return (
    <g>
      {mills.map((m) => {
        const pos = fl.millPositions[m.id];
        if (!pos) return null;
        const fill = STATE_FILL[m.state] || STATE_FILL.Idle;
        const selected = selectedMill === m.id;
        const faulted = m.state === "Faulted";

        return (
          <g
            key={m.id}
            onClick={() => onSelectMill(selected ? null : m.id)}
            style={{ cursor: "pointer" }}
          >
            <rect
              x={pos.x - rw / 2}
              y={pos.y - rh / 2}
              width={rw}
              height={rh}
              rx={4}
              fill={fill}
              stroke={selected ? "var(--accent)" : faulted ? "var(--red)" : "none"}
              strokeWidth={selected ? 2 : faulted ? 1.5 : 0}
              style={faulted ? { animation: "pulse-red 1.5s infinite" } : undefined}
            />
            <text
              x={pos.x}
              y={pos.y - 4}
              textAnchor="middle"
              fill="var(--text)"
              fontSize={10}
              fontWeight={600}
            >
              M{m.id}
            </text>
            <text
              x={pos.x}
              y={pos.y + 10}
              textAnchor="middle"
              fill="var(--text-dim)"
              fontSize={7}
            >
              {m.state}
            </text>
          </g>
        );
      })}
    </g>
  );
}
