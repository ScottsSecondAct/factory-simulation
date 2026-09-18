import { useMemo } from "react";
import { useStore } from "../../store";
import type { FactoryLayout } from "../../types";
import { computeFloorLayout, VIEW_BOX } from "./layout";
import { LaneNetwork } from "./LaneNetwork";
import { MillGrid } from "./MillGrid";
import { AgvLayer } from "./AgvLayer";

interface Props {
  layout: FactoryLayout;
}

export function FactoryFloor({ layout }: Props) {
  const snapshot = useStore((s) => s.snapshot);
  const selectedMill = useStore((s) => s.selectedMill);
  const setSelectedMill = useStore((s) => s.setSelectedMill);

  const fl = useMemo(() => computeFloorLayout(layout), [layout]);

  if (!snapshot) {
    return (
      <div style={styles.container}>
        <div style={styles.placeholder}>Waiting for simulation...</div>
      </div>
    );
  }

  const loopSegs = layout.mills.length > 0
    ? Math.max(...layout.mills.map((m) => m.loop_seg)) + 4
    : 20;

  return (
    <div style={styles.container}>
      <svg viewBox={VIEW_BOX} style={styles.svg}>
        <LaneNetwork fl={fl} laneOccupancy={snapshot.lane_occupancy} />

        {/* Station markers */}
        <g>
          <rect
            x={fl.stationPositions.toolCrib.x - 14}
            y={fl.stationPositions.toolCrib.y - 8}
            width={28}
            height={16}
            rx={3}
            fill="var(--station-tools-bg)"
            stroke="var(--station-tools-border)"
            strokeWidth={1}
          />
          <text
            x={fl.stationPositions.toolCrib.x}
            y={fl.stationPositions.toolCrib.y + 4}
            textAnchor="middle"
            fill="var(--yellow)"
            fontSize={7}
            fontWeight={600}
          >
            TOOLS
          </text>
        </g>
        <g>
          <rect
            x={fl.stationPositions.palletMag.x - 14}
            y={fl.stationPositions.palletMag.y - 8}
            width={28}
            height={16}
            rx={3}
            fill="var(--station-pallets-bg)"
            stroke="var(--station-pallets-border)"
            strokeWidth={1}
          />
          <text
            x={fl.stationPositions.palletMag.x}
            y={fl.stationPositions.palletMag.y + 4}
            textAnchor="middle"
            fill="var(--accent)"
            fontSize={7}
            fontWeight={600}
          >
            PALLETS
          </text>
        </g>
        <g>
          <rect
            x={fl.stationPositions.workPrep.x - 14}
            y={fl.stationPositions.workPrep.y - 8}
            width={28}
            height={16}
            rx={3}
            fill="var(--station-prep-bg)"
            stroke="var(--station-prep-border)"
            strokeWidth={1}
          />
          <text
            x={fl.stationPositions.workPrep.x}
            y={fl.stationPositions.workPrep.y + 4}
            textAnchor="middle"
            fill="var(--station-prep-text)"
            fontSize={6}
            fontWeight={600}
          >
            PREP
          </text>
        </g>

        <MillGrid
          mills={snapshot.mills}
          fl={fl}
          selectedMill={selectedMill}
          onSelectMill={setSelectedMill}
        />

        <AgvLayer
          agvs={snapshot.agvs}
          fl={fl}
          loopSegments={loopSegs}
        />
      </svg>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  container: {
    flex: 1,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: 12,
    overflow: "hidden",
  },
  svg: {
    width: "100%",
    height: "100%",
    maxHeight: "100%",
  },
  placeholder: {
    color: "var(--text-dim)",
    fontSize: 14,
  },
};
