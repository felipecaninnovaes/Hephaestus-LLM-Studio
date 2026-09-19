"use client";

import type { JobMetrics } from "@/types/jobs";
import type { ChartCurves, ChartTab } from "./chartMath";

export interface ConvergenceChartCanvasProps {
  curves: ChartCurves | null;
  metrics: JobMetrics[];
  tab: ChartTab;
  hoverIndex: number | null;
  setHoverIndex: (idx: number | null) => void;
  totalEpochs?: number;
  viewBoxWidth: number;
  viewBoxHeight: number;
  padLeft: number;
  padRight: number;
  padTop: number;
  padBottom: number;
  graphWidth: number;
  graphHeight: number;
}

export function ConvergenceChartCanvas({
  curves,
  metrics,
  tab,
  hoverIndex,
  setHoverIndex,
  totalEpochs,
  viewBoxWidth,
  viewBoxHeight,
  padLeft,
  padRight,
  padTop,
  graphWidth,
  graphHeight,
}: ConvergenceChartCanvasProps) {
  return (
    <div className="relative select-none">
      <svg
        viewBox={`0 0 ${viewBoxWidth} ${viewBoxHeight}`}
        className="w-full h-44 overflow-visible block"
        role="img"
        aria-label="Gráfico vetorial de curvas de convergência de treino"
        onMouseLeave={() => setHoverIndex(null)}
        onMouseMove={(e) => {
          if (metrics.length <= 1) return;
          const rect = e.currentTarget.getBoundingClientRect();
          const relX = ((e.clientX - rect.left) / rect.width) * viewBoxWidth;
          const clampedX = Math.max(
            padLeft,
            Math.min(viewBoxWidth - padRight, relX),
          );
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
                  className="font-mono text-2xs fill-zinc-400"
                >
                  {(
                    curves.maxLoss -
                    pct * (curves.maxLoss - curves.minLoss)
                  ).toFixed(1)}
                </text>
              )}
              {tab === "map" && (
                <text
                  x={padLeft - 6}
                  y={y + 3}
                  textAnchor="end"
                  className="font-mono text-2xs fill-zinc-400"
                >
                  {Math.round((1 - pct) * 100)}%
                </text>
              )}
            </g>
          );
        })}

        {/* Defs para gradientes de preenchimento */}
        <defs>
          <linearGradient id="diffLossGradient" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#818cf8" stopOpacity="0.4" />
            <stop offset="100%" stopColor="#818cf8" stopOpacity="0.0" />
          </linearGradient>
        </defs>

        {/* Curva de Perda Difusão LoRA */}
        {curves?.isDiffusion && (
          <>
            {metrics.length > 1 && (
              <path
                d={`${curves.diffLoss.path} L ${curves.diffLoss.coords[curves.diffLoss.coords.length - 1].x} ${padTop + graphHeight} L ${curves.diffLoss.coords[0].x} ${padTop + graphHeight} Z`}
                fill="url(#diffLossGradient)"
              />
            )}
            <path
              d={curves.diffLoss.path}
              fill="none"
              stroke="#818cf8"
              strokeWidth={2.25}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </>
        )}

        {/* Curvas de Perda YOLO (Losses) */}
        {curves && !curves.isDiffusion && (tab === "all" || tab === "loss") && (
          <>
            <path
              d={curves.boxLoss.path}
              fill="none"
              stroke="#38bdf8"
              strokeWidth={1.75}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
            <path
              d={curves.clsLoss.path}
              fill="none"
              stroke="#818cf8"
              strokeWidth={1.75}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
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

        {/* Curvas de Precisão YOLO (mAP) */}
        {curves && !curves.isDiffusion && (tab === "all" || tab === "map") && (
          <>
            <path
              d={curves.map50.path}
              fill="none"
              stroke="#34d399"
              strokeWidth={2}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
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
            <line
              x1={padLeft + (hoverIndex / (metrics.length - 1)) * graphWidth}
              y1={padTop}
              x2={padLeft + (hoverIndex / (metrics.length - 1)) * graphWidth}
              y2={padTop + graphHeight}
              stroke="rgba(255,255,255,0.3)"
              strokeWidth={1}
              strokeDasharray="3 3"
            />
            {curves.isDiffusion ? (
              <circle
                cx={curves.diffLoss.coords[hoverIndex]?.x}
                cy={curves.diffLoss.coords[hoverIndex]?.y}
                r={4}
                fill="#818cf8"
              />
            ) : (
              <>
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
              </>
            )}
          </g>
        )}

        {/* Marcador único para quando só há 1 checkpoint */}
        {curves && metrics.length === 1 && (
          <g>
            {curves.isDiffusion ? (
              <circle
                cx={curves.diffLoss.coords[0].x}
                cy={curves.diffLoss.coords[0].y}
                r={5}
                fill="#818cf8"
              />
            ) : (
              <>
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
              </>
            )}
          </g>
        )}

        {/* Eixo X: Marcadores de Época */}
        <text
          x={padLeft}
          y={viewBoxHeight - 6}
          textAnchor="start"
          className="font-mono text-2xs fill-zinc-400"
        >
          Epoch {metrics[0].epoch}
        </text>
        <text
          x={viewBoxWidth - padRight}
          y={viewBoxHeight - 6}
          textAnchor="end"
          className="font-mono text-2xs fill-zinc-400"
        >
          Epoch {metrics[metrics.length - 1].epoch}
          {totalEpochs && totalEpochs > metrics[metrics.length - 1].epoch
            ? ` / ${totalEpochs}`
            : ""}
        </text>
      </svg>
    </div>
  );
}
