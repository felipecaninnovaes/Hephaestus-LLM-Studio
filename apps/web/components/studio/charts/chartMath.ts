import type { JobMetrics } from "@/types/jobs";

export type ChartTab = "all" | "loss" | "map";

export interface Coord {
  x: number;
  y: number;
  value: number;
}

export interface CurveData {
  coords: Coord[];
  path: string;
}

export interface ChartCurves {
  epochs: number[];
  maxEpoch: number;
  minLoss: number;
  maxLoss: number;
  minMap: number;
  maxMap: number;
  isDiffusion: boolean;
  diffLoss: CurveData;
  boxLoss: CurveData;
  clsLoss: CurveData;
  dflLoss: CurveData;
  map50: CurveData;
  map5095: CurveData;
}

export function toCoords(
  values: number[],
  minVal: number,
  maxVal: number,
  count: number,
  padLeft: number,
  padTop: number,
  graphWidth: number,
  graphHeight: number,
): Coord[] {
  const range = maxVal - minVal || 1;
  return values.map((v, i) => {
    const x =
      count === 1
        ? padLeft + graphWidth / 2
        : padLeft + (i / (count - 1)) * graphWidth;
    const y = padTop + graphHeight - ((v - minVal) / range) * graphHeight;
    return { x, y, value: v };
  });
}

export function toPath(coords: { x: number; y: number }[]): string {
  if (coords.length === 0) return "";
  if (coords.length === 1) return `M ${coords[0].x} ${coords[0].y}`;
  return coords.reduce(
    (acc, pt, i) =>
      `${acc} ${i === 0 ? "M" : "L"} ${pt.x.toFixed(1)} ${pt.y.toFixed(1)}`,
    "",
  );
}

export function calculateCurves(
  metrics: JobMetrics[],
  totalEpochs: number | undefined,
  padLeft: number,
  padTop: number,
  graphWidth: number,
  graphHeight: number,
): ChartCurves | null {
  if (!metrics || metrics.length === 0) return null;

  const epochs = metrics.map((m) => m.epoch);
  const maxEpoch = totalEpochs || Math.max(...epochs, 1);
  const count = metrics.length;

  const isDiffusion = metrics.some(
    (m) => m.loss != null && (m.boxLoss === 0 || m.boxLoss == null),
  );

  const allLosses = isDiffusion
    ? metrics.map((m) => m.loss ?? 0)
    : metrics.flatMap((m) => [m.boxLoss ?? 0, m.clsLoss ?? 0, m.dflLoss ?? 0]);
  const minLoss = Math.max(0, Math.min(...allLosses) * 0.9);
  const maxLoss = Math.max(...allLosses, 0.5) * 1.05;

  const minMap = 0;
  const maxMap = Math.max(
    1,
    Math.max(...metrics.flatMap((m) => [m.map50 ?? 0, m.map5095 ?? 0])) * 1.1,
  );

  const diffLossCoords = toCoords(
    metrics.map((m) => m.loss ?? 0),
    minLoss,
    maxLoss,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );
  const boxLossCoords = toCoords(
    metrics.map((m) => m.boxLoss ?? 0),
    minLoss,
    maxLoss,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );
  const clsLossCoords = toCoords(
    metrics.map((m) => m.clsLoss ?? 0),
    minLoss,
    maxLoss,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );
  const dflLossCoords = toCoords(
    metrics.map((m) => m.dflLoss ?? 0),
    minLoss,
    maxLoss,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );
  const map50Coords = toCoords(
    metrics.map((m) => m.map50 ?? 0),
    minMap,
    maxMap,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );
  const map5095Coords = toCoords(
    metrics.map((m) => m.map5095 ?? 0),
    minMap,
    maxMap,
    count,
    padLeft,
    padTop,
    graphWidth,
    graphHeight,
  );

  return {
    epochs,
    maxEpoch,
    minLoss,
    maxLoss,
    minMap,
    maxMap,
    isDiffusion,
    diffLoss: { coords: diffLossCoords, path: toPath(diffLossCoords) },
    boxLoss: { coords: boxLossCoords, path: toPath(boxLossCoords) },
    clsLoss: { coords: clsLossCoords, path: toPath(clsLossCoords) },
    dflLoss: { coords: dflLossCoords, path: toPath(dflLossCoords) },
    map50: { coords: map50Coords, path: toPath(map50Coords) },
    map5095: { coords: map5095Coords, path: toPath(map5095Coords) },
  };
}
