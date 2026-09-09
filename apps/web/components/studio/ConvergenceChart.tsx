"use client";

import { useId, useMemo, useState } from "react";
import type { JobMetrics } from "@/types/studio";
import { IconActivity, IconTrendingUp } from "@/components/icons";

interface SparklineProps {
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
      (acc, pt, i) => `${acc} ${i === 0 ? "M" : "L"} ${pt.x.toFixed(1)} ${pt.y.toFixed(1)}`,
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
            <stop offset="0%" stopColor={fillColor || color} stopOpacity={0.25} />
            <stop offset="100%" stopColor={fillColor || color} stopOpacity={0.0} />
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

type ChartTab = "all" | "loss" | "map";

interface ConvergenceChartProps {
  metrics: JobMetrics[];
  totalEpochs?: number;
  isJobActive?: boolean;
}

export function ConvergenceChart({
  metrics,
  totalEpochs,
  isJobActive = false,
}: ConvergenceChartProps) {
  const [tab, setTab] = useState<ChartTab>("all");
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const viewBoxWidth = 600;
  const viewBoxHeight = 160;
  const padLeft = 40;
  const padRight = 16;
  const padTop = 14;
  const padBottom = 26;
  const graphWidth = viewBoxWidth - padLeft - padRight;
  const graphHeight = viewBoxHeight - padTop - padBottom;

  const curves = useMemo(() => {
    if (!metrics || metrics.length === 0) return null;

    const epochs = metrics.map((m) => m.epoch);
    const maxEpoch = totalEpochs || Math.max(...epochs, 1);
    const count = metrics.length;

    // Helper de cálculo de coordenadas
    function toCoords(values: number[], minVal: number, maxVal: number) {
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

    // Perdas (Losses) compartilham escala de perda
    const allLosses = metrics.flatMap((m) => [m.boxLoss, m.clsLoss, m.dflLoss]);
    const minLoss = Math.max(0, Math.min(...allLosses) * 0.9);
    const maxLoss = Math.max(...allLosses, 0.5) * 1.05;

    // Precisões mAP operam em 0..1 (0%..100%)
    const minMap = 0;
    const maxMap = Math.max(
      1,
      Math.max(...metrics.flatMap((m) => [m.map50, m.map5095])) * 1.1,
    );

    const boxLossCoords = toCoords(
      metrics.map((m) => m.boxLoss),
      minLoss,
      maxLoss,
    );
    const clsLossCoords = toCoords(
      metrics.map((m) => m.clsLoss),
      minLoss,
      maxLoss,
    );
    const dflLossCoords = toCoords(
      metrics.map((m) => m.dflLoss),
      minLoss,
      maxLoss,
    );
    const map50Coords = toCoords(
      metrics.map((m) => m.map50),
      minMap,
      maxMap,
    );
    const map5095Coords = toCoords(
      metrics.map((m) => m.map5095),
      minMap,
      maxMap,
    );

    function toPath(coords: { x: number; y: number }[]) {
      if (coords.length === 0) return "";
      if (coords.length === 1) return `M ${coords[0].x} ${coords[0].y}`;
      return coords.reduce(
        (acc, pt, i) => `${acc} ${i === 0 ? "M" : "L"} ${pt.x.toFixed(1)} ${pt.y.toFixed(1)}`,
        "",
      );
    }

    return {
      epochs,
      maxEpoch,
      minLoss,
      maxLoss,
      minMap,
      maxMap,
      boxLoss: { coords: boxLossCoords, path: toPath(boxLossCoords) },
      clsLoss: { coords: clsLossCoords, path: toPath(clsLossCoords) },
      dflLoss: { coords: dflLossCoords, path: toPath(dflLossCoords) },
      map50: { coords: map50Coords, path: toPath(map50Coords) },
      map5095: { coords: map5095Coords, path: toPath(map5095Coords) },
    };
  }, [metrics, totalEpochs, graphWidth, graphHeight]);

  if (!metrics || metrics.length === 0) {
    return (
      <div className="rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm p-6 text-center">
        <div className="mx-auto flex size-10 items-center justify-center rounded-lg border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm text-zinc-400">
          <IconActivity className="size-5" />
        </div>
        <h4 className="mt-3 font-display text-sm font-semibold text-zinc-200">
          Aguardando Telemetria de Treino
        </h4>
        <p className="mt-1 font-mono text-[11px] text-zinc-400">
          O orquestrador enviará os primeiros checkpoints assim que a época 1 for
          processada.
        </p>
      </div>
    );
  }

  const latest = metrics[metrics.length - 1];
  const activeHover = hoverIndex != null && metrics[hoverIndex] ? metrics[hoverIndex] : latest;

  return (
    <div className="glass-card rounded-xl shadow-lg p-4 space-y-3.5">
      {/* Header do Gráfico com Tabs e Indicador de Época */}
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        <div className="flex items-center gap-2">
          <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-brand-500/10 text-brand-400 border border-brand-500/20 backdrop-blur-sm">
            <IconTrendingUp className="size-3.5" />
          </div>
          <span className="font-mono text-[11px] font-semibold tracking-caps uppercase text-zinc-200 whitespace-nowrap">
            Curvas de Convergência
          </span>
          <span className="font-mono text-[11px] text-zinc-400 whitespace-nowrap">
            ({metrics.length} checkpoint{metrics.length > 1 ? "s" : ""})
          </span>
          {isJobActive && (
            <span className="flex items-center gap-1 font-mono text-[11px] text-brand-400 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
              Live
            </span>
          )}
        </div>

        {/* Tab Switcher Segmentado */}
        <div className="flex items-center rounded-lg border border-white/10 bg-black/40 backdrop-blur-sm p-0.5">
          <button
            type="button"
            onClick={() => setTab("all")}
            className={`rounded-md px-2.5 py-1 font-mono text-[11px] transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
              tab === "all"
                ? "bg-zinc-800 text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            Todas
          </button>
          <button
            type="button"
            onClick={() => setTab("loss")}
            className={`rounded-md px-2.5 py-1 font-mono text-[11px] transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
              tab === "loss"
                ? "bg-zinc-800 text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            Perdas (Loss)
          </button>
          <button
            type="button"
            onClick={() => setTab("map")}
            className={`rounded-md px-2.5 py-1 font-mono text-[11px] transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
              tab === "map"
                ? "bg-zinc-800 text-zinc-100 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            Precisão (mAP)
          </button>
        </div>
      </div>

      {/* Canvas Vetorial do Gráfico */}
      <div className="relative select-none">
        <svg
          viewBox={`0 0 ${viewBoxWidth} ${viewBoxHeight}`}
          className="w-full h-44 overflow-visible block"
          role="img"
          aria-label="Gráfico vetorial de curvas de convergência de treino YOLO"
          onMouseLeave={() => setHoverIndex(null)}
          onMouseMove={(e) => {
            if (metrics.length <= 1) return;
            const rect = e.currentTarget.getBoundingClientRect();
            const relX = ((e.clientX - rect.left) / rect.width) * viewBoxWidth;
            const clampedX = Math.max(padLeft, Math.min(viewBoxWidth - padRight, relX));
            const progress = (clampedX - padLeft) / graphWidth;
            const idx = Math.round(progress * (metrics.length - 1));
            setHoverIndex(idx);
          }}
        >
          {/* Linhas de Grade Horizontal (25%, 50%, 75%) */}
          {[0, 0.25, 0.5, 0.75, 1].map((pct) => {
            const y = padTop + graphHeight * pct;
            return (
              <g key={pct}>
                <line
                  x1={padLeft}
                  y1={y}
                  x2={viewBoxWidth - padRight}
                  y2={y}
                  stroke="rgba(255,255,255,0.06)"
                  strokeDasharray="2 3"
                />
                {tab !== "map" && curves && (
                  <text
                    x={padLeft - 6}
                    y={y + 3}
                    textAnchor="end"
                    className="font-mono text-[11px] fill-zinc-400"
                  >
                    {(curves.maxLoss - pct * (curves.maxLoss - curves.minLoss)).toFixed(1)}
                  </text>
                )}
                {tab === "map" && (
                  <text
                    x={padLeft - 6}
                    y={y + 3}
                    textAnchor="end"
                    className="font-mono text-[11px] fill-zinc-400"
                  >
                    {Math.round((1 - pct) * 100)}%
                  </text>
                )}
              </g>
            );
          })}

          {/* Curvas de Perda (Losses) */}
          {curves && (tab === "all" || tab === "loss") && (
            <>
              {/* Box Loss (Sky/Cyan) */}
              <path
                d={curves.boxLoss.path}
                fill="none"
                stroke="#38bdf8"
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              {/* Cls Loss (Indigo/Violet) */}
              <path
                d={curves.clsLoss.path}
                fill="none"
                stroke="#818cf8"
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              {/* DFL Loss (Amber) */}
              <path
                d={curves.dflLoss.path}
                fill="none"
                stroke="#fbbf24"
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </>
          )}

          {/* Curvas de Precisão (mAP) */}
          {curves && (tab === "all" || tab === "map") && (
            <>
              {/* mAP@50 (Brand Green #34d399) */}
              <path
                d={curves.map50.path}
                fill="none"
                stroke="#34d399"
                strokeWidth={2}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
              {/* mAP@50-95 (Teal) */}
              <path
                d={curves.map5095.path}
                fill="none"
                stroke="#2dd4bf"
                strokeWidth={1.75}
                strokeDasharray="4 2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </>
          )}

          {/* Ponto ou Crosshair Hover */}
          {curves && hoverIndex != null && metrics.length > 1 && (
            <g>
              {/* Linha vertical indicadora */}
              <line
                x1={padLeft + (hoverIndex / (metrics.length - 1)) * graphWidth}
                y1={padTop}
                x2={padLeft + (hoverIndex / (metrics.length - 1)) * graphWidth}
                y2={padTop + graphHeight}
                stroke="rgba(255,255,255,0.3)"
                strokeWidth={1}
                strokeDasharray="3 3"
              />
              {/* Pontos nas interseções */}
              {(tab === "all" || tab === "loss") && (
                <>
                  <circle
                    cx={curves.boxLoss.coords[hoverIndex]?.x}
                    cy={curves.boxLoss.coords[hoverIndex]?.y}
                    r={3.5}
                    fill="#38bdf8"
                  />
                  <circle
                    cx={curves.clsLoss.coords[hoverIndex]?.x}
                    cy={curves.clsLoss.coords[hoverIndex]?.y}
                    r={3.5}
                    fill="#818cf8"
                  />
                  <circle
                    cx={curves.dflLoss.coords[hoverIndex]?.x}
                    cy={curves.dflLoss.coords[hoverIndex]?.y}
                    r={3.5}
                    fill="#fbbf24"
                  />
                </>
              )}
              {(tab === "all" || tab === "map") && (
                <>
                  <circle
                    cx={curves.map50.coords[hoverIndex]?.x}
                    cy={curves.map50.coords[hoverIndex]?.y}
                    r={4}
                    fill="#34d399"
                  />
                  <circle
                    cx={curves.map5095.coords[hoverIndex]?.x}
                    cy={curves.map5095.coords[hoverIndex]?.y}
                    r={3.5}
                    fill="#2dd4bf"
                  />
                </>
              )}
            </g>
          )}

          {/* Marcador único para quando só há 1 checkpoint */}
          {curves && metrics.length === 1 && (
            <g>
              {(tab === "all" || tab === "loss") && (
                <>
                  <circle
                    cx={curves.boxLoss.coords[0].x}
                    cy={curves.boxLoss.coords[0].y}
                    r={4}
                    fill="#38bdf8"
                  />
                  <circle
                    cx={curves.clsLoss.coords[0].x}
                    cy={curves.clsLoss.coords[0].y}
                    r={4}
                    fill="#818cf8"
                  />
                  <circle
                    cx={curves.dflLoss.coords[0].x}
                    cy={curves.dflLoss.coords[0].y}
                    r={4}
                    fill="#fbbf24"
                  />
                </>
              )}
              {(tab === "all" || tab === "map") && (
                <circle
                  cx={curves.map50.coords[0].x}
                  cy={curves.map50.coords[0].y}
                  r={5}
                  fill="#34d399"
                />
              )}
            </g>
          )}

          {/* Eixo X: Marcadores de Época */}
          <text
            x={padLeft}
            y={viewBoxHeight - 6}
            textAnchor="start"
            className="font-mono text-[11px] fill-zinc-400"
          >
            Epoch {metrics[0].epoch}
          </text>
          <text
            x={viewBoxWidth - padRight}
            y={viewBoxHeight - 6}
            textAnchor="end"
            className="font-mono text-[11px] fill-zinc-400"
          >
            Epoch {metrics[metrics.length - 1].epoch}
            {totalEpochs && totalEpochs > metrics[metrics.length - 1].epoch
              ? ` / ${totalEpochs}`
              : ""}
          </text>
        </svg>
      </div>

      {/* Legenda Dinâmica com Valores do Ponto Focado / Mais Recente */}
      <div className="flex flex-wrap items-center justify-between gap-3 pt-1 border-t border-zinc-800/80 font-mono text-[11px]">
        <div className="text-zinc-400 flex items-center gap-1.5">
          <span>Leitura:</span>
          <span className="text-zinc-200 font-semibold">Epoch {activeHover.epoch}</span>
          {hoverIndex != null && hoverIndex !== metrics.length - 1 && (
            <span className="text-brand-300">(Histórico)</span>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-3">
          {(tab === "all" || tab === "map") && (
            <>
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-[#34d399]" />
                <span className="text-zinc-400">mAP@50:</span>
                <span className="font-semibold text-zinc-100">
                  {(activeHover.map50 * 100).toFixed(1)}%
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-[#2dd4bf]" />
                <span className="text-zinc-400">mAP@50-95:</span>
                <span className="font-semibold text-zinc-100">
                  {(activeHover.map5095 * 100).toFixed(1)}%
                </span>
              </div>
            </>
          )}

          {(tab === "all" || tab === "loss") && (
            <>
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-[#38bdf8]" />
                <span className="text-zinc-400">Box Loss:</span>
                <span className="font-semibold text-zinc-100">
                  {activeHover.boxLoss.toFixed(4)}
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-[#818cf8]" />
                <span className="text-zinc-400">Cls Loss:</span>
                <span className="font-semibold text-zinc-100">
                  {activeHover.clsLoss.toFixed(4)}
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-[#fbbf24]" />
                <span className="text-zinc-400">DFL Loss:</span>
                <span className="font-semibold text-zinc-100">
                  {activeHover.dflLoss.toFixed(4)}
                </span>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
