import type { MillSnap, MillId } from "../../types";
import type { FloorLayout } from "./layout";
import { CELL_W, CELL_H } from "./layout";

const STATE_VAR: Record<string, string> = {
  Idle: "var(--mill-idle)",
  Machining: "var(--mill-machining)",
  Loading: "var(--mill-loading)",
  Unloading: "var(--mill-loading)",
  ToolChange: "var(--mill-toolchange)",
  WaitingPallet: "var(--mill-waiting)",
  WaitingTool: "var(--mill-waiting)",
  Faulted: "var(--mill-faulted)",
  ChipFull: "var(--mill-chipfull)",
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
        const fill = STATE_VAR[m.state] || STATE_VAR.Idle;
        const selected = selectedMill === m.id;
        const faulted = m.state === "Faulted";
        const chipFull = m.state === "ChipFull";
        const chipPct = m.chip_capacity > 0 ? m.chip_level / m.chip_capacity : 0;
        const chipBarW = rw - 8;

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
              stroke={selected ? "var(--accent)" : faulted ? "var(--red)" : chipFull ? "#f59e0b" : "none"}
              strokeWidth={selected ? 2 : faulted ? 1.5 : chipFull ? 1.5 : 0}
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
            {/* Chip level indicator bar */}
            <rect
              x={pos.x - chipBarW / 2}
              y={pos.y + rh / 2 - 5}
              width={chipBarW}
              height={3}
              rx={1}
              fill="var(--chip-bar-bg)"
            />
            {chipPct > 0 && (
              <rect
                x={pos.x - chipBarW / 2}
                y={pos.y + rh / 2 - 5}
                width={chipBarW * Math.min(chipPct, 1)}
                height={3}
                rx={1}
                fill={chipPct >= 1 ? "#f59e0b" : "#6b7280"}
              />
            )}
          </g>
        );
      })}
    </g>
  );
}
