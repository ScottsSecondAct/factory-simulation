import type { FloorLayout } from "./layout";

interface Props {
  fl: FloorLayout;
  laneOccupancy: number[];
}

export function LaneNetwork({ fl, laneOccupancy }: Props) {
  const segs: React.ReactNode[] = [];

  for (let i = 0; i < fl.loopPoints.length; i++) {
    const a = fl.loopPoints[i];
    const b = fl.loopPoints[(i + 1) % fl.loopPoints.length];
    const occupied = laneOccupancy[i] >= 0;
    segs.push(
      <line
        key={`loop-${i}`}
        x1={a.x}
        y1={a.y}
        x2={b.x}
        y2={b.y}
        stroke={occupied ? "#93c5fd" : "#cbd5e1"}
        strokeWidth={occupied ? 3 : 2}
        strokeLinecap="round"
      />
    );
  }

  for (const spur of fl.spurLines) {
    const occupied = laneOccupancy[spur.seg] >= 0;
    segs.push(
      <line
        key={`spur-${spur.seg}`}
        x1={spur.from.x}
        y1={spur.from.y}
        x2={spur.to.x}
        y2={spur.to.y}
        stroke={occupied ? "#93c5fd" : "#cbd5e1"}
        strokeWidth={1.5}
        strokeDasharray="4 3"
        strokeLinecap="round"
      />
    );
  }

  return <g>{segs}</g>;
}
