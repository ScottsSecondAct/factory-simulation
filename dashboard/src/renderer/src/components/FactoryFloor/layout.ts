import type { FactoryLayout, MillLayout, SegmentId } from "../../types";

export interface Point {
  x: number;
  y: number;
}

export interface FloorLayout {
  loopPoints: Point[];
  millPositions: Point[];
  spurLines: { from: Point; to: Point; seg: SegmentId }[];
  stationPositions: { toolCrib: Point; palletMag: Point; workPrep: Point };
  loopSegments: number;
}

const VIEW_W = 800;
const VIEW_H = 500;
const CX = VIEW_W / 2;
const CY = VIEW_H / 2 - 20;
const RX = 340;
const RY = 200;

const GRID_W = 280;
const GRID_H = 200;
const GRID_X = CX - GRID_W / 2;
const GRID_Y = CY - GRID_H / 2;

export const CELL_W = GRID_W / 5;
export const CELL_H = GRID_H / 5;

export function computeFloorLayout(layout: FactoryLayout): FloorLayout {
  const loopSegs = layout.mills.length > 0
    ? Math.max(...layout.mills.map((m) => m.loop_seg)) + 4
    : 20;

  const loopPoints: Point[] = Array.from({ length: loopSegs }, (_, i) => {
    const angle = (i / loopSegs) * Math.PI * 2 - Math.PI / 2;
    return { x: CX + RX * Math.cos(angle), y: CY + RY * Math.sin(angle) };
  });

  const millPositions: Point[] = layout.mills.map((m) => ({
    x: GRID_X + m.col * CELL_W + CELL_W / 2,
    y: GRID_Y + m.row * CELL_H + CELL_H / 2,
  }));

  const spurLines = layout.mills.map((m) => ({
    from: loopPoints[m.loop_seg] || { x: CX, y: CY },
    to: millPositions[m.id],
    seg: m.spur_seg,
  }));

  const tcSeg = layout.stations.tool_crib;
  const pmSeg = layout.stations.pallet_magazine;
  const wpSeg = layout.stations.work_prep;

  return {
    loopPoints,
    millPositions,
    spurLines,
    stationPositions: {
      toolCrib: loopPoints[tcSeg] || { x: 0, y: 0 },
      palletMag: loopPoints[pmSeg] || { x: VIEW_W, y: VIEW_H },
      workPrep: loopPoints[wpSeg] || { x: VIEW_W / 2, y: VIEW_H },
    },
    loopSegments: loopSegs,
  };
}

export function segmentCenter(
  segId: SegmentId,
  fl: FloorLayout,
  loopSegs: number
): Point {
  if (segId < loopSegs) {
    return fl.loopPoints[segId] || { x: CX, y: CY };
  }
  const millIdx = segId - loopSegs;
  return fl.millPositions[millIdx] || { x: CX, y: CY };
}

export const VIEW_BOX = `0 0 ${VIEW_W} ${VIEW_H}`;
