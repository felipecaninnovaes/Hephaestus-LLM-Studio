"use client";

import { useId, useMemo } from "react";

export interface SparklineProps {
  data: number[];
  color: string;
  fillColor?: string;
  height?: number;
  width?: number;
}

/**
 * Mini curva vetorial SVG para os cards individuais de métricas.
 */
export function MetricSparkline({
  data,
  color,
  fillColor,
  height = 28,
  width = 120,
}: SparklineProps) {
  const gradientId = useId();

  const points = useMemo(() => {
    if (!data || data.length === 0) return null;
    if (data.length === 1) {
      const y = height / 2;
      return {
        linePath: `M 0 ${y} L ${width} ${y}`,
        areaPath: `M 0 ${y} L ${width} ${y} L ${width} ${height} L 0 ${height} Z`,
        lastPoint: { x: width, y },
      };
    }

    const min = Math.min(...data);
    const max = Math.max(...data);
    const range = max - min || 1;
    const padding = 4;
    const effHeight = height - padding * 2;

    const coords = data.map((v, i) => {
      const x = (i / (data.length - 1)) * width;
      const y = height - padding - ((v - min) / range) * effHeight;
      return { x, y };
    });

    const linePath = coords.reduce(
      (acc, pt, i) =>
        `${acc} ${i === 0 ? "M" : "L"} ${pt.x.toFixed(1)} ${pt.y.toFixed(1)}`,
      "",
    );

    const first = coords[0];
    const last = coords[coords.length - 1];
    const areaPath = `${linePath} L ${last.x.toFixed(1)} ${height} L ${first.x.toFixed(1)} ${height} Z`;

    return {
      linePath,
      areaPath,
      lastPoint: last,
    };
  }, [data, height, width]);

  if (!points) {
    return (
      <div className="h-7 w-full flex items-center justify-center">
        <div className="h-0.5 w-full border-b border-dashed border-zinc-800" />
      </div>
    );
  }

  return (
    <div className="w-full overflow-hidden">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        className="w-full h-7 overflow-visible block"
        preserveAspectRatio="none"
        aria-hidden="true"
        role="img"
      >
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop
              offset="0%"
              stopColor={fillColor || color}
              stopOpacity={0.25}
            />
            <stop
              offset="100%"
              stopColor={fillColor || color}
              stopOpacity={0.0}
            />
          </linearGradient>
        </defs>
        <path d={points.areaPath} fill={`url(#${gradientId})`} />
        <path
          d={points.linePath}
          fill="none"
          stroke={color}
          strokeWidth={1.5}
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <circle
          cx={points.lastPoint.x}
          cy={points.lastPoint.y}
          r={2.5}
          fill={color}
          className="animate-pulse motion-reduce:animate-none"
        />
      </svg>
    </div>
  );
}
