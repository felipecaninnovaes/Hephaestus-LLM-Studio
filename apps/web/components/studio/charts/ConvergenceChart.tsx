"use client";

import { useMemo, useState } from "react";
import type { JobMetrics } from "@/types/jobs";
import { trainingMetrics } from "@/lib/jobMetrics";
import { IconActivity, IconTrendingUp } from "@/components/icons";
import { calculateCurves, type ChartTab } from "./chartMath";
import { ConvergenceChartCanvas } from "./ConvergenceChartCanvas";
import { ConvergenceMetricCards } from "./ConvergenceMetricCards";

export interface ConvergenceChartProps {
  metrics: JobMetrics[];
  totalEpochs?: number;
  isJobActive?: boolean;
}

export function ConvergenceChart({
  metrics: rawMetrics,
  totalEpochs,
  isJobActive = false,
}: ConvergenceChartProps) {
  // AC-006-B: linhas de status/boot do engine não são pontos de treino.
  const metrics = useMemo(() => trainingMetrics(rawMetrics), [rawMetrics]);
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

  const curves = useMemo(
    () =>
      calculateCurves(
        metrics,
        totalEpochs,
        padLeft,
        padTop,
        graphWidth,
        graphHeight,
      ),
    [metrics, totalEpochs, graphWidth, graphHeight],
  );

  if (!metrics || metrics.length === 0) {
    return (
      <div className="rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm p-6 text-center">
        <div className="mx-auto flex size-10 items-center justify-center rounded-lg border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm text-zinc-400">
          <IconActivity className="size-5" />
        </div>
        <h4 className="mt-3 font-display text-sm font-semibold text-zinc-200">
          Aguardando Telemetria de Treino
        </h4>
        <p className="mt-1 font-mono text-2xs text-zinc-400">
          O orquestrador enviará os primeiros checkpoints assim que a época 1
          for processada.
        </p>
      </div>
    );
  }

  const latest = metrics[metrics.length - 1];
  const activeHover =
    hoverIndex != null && metrics[hoverIndex] ? metrics[hoverIndex] : latest;

  return (
    <div className="glass-card rounded-xl shadow-lg p-4 space-y-3.5">
      {/* Header do Gráfico com Tabs e Indicador de Época */}
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        <div className="flex items-center gap-2">
          <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-brand-500/10 text-brand-400 border border-brand-500/20 backdrop-blur-sm">
            <IconTrendingUp className="size-3.5" />
          </div>
          <span className="font-mono text-2xs font-semibold tracking-caps uppercase text-zinc-200 whitespace-nowrap">
            Curvas de Convergência
          </span>
          <span className="font-mono text-2xs text-zinc-400 whitespace-nowrap">
            ({metrics.length} checkpoint{metrics.length > 1 ? "s" : ""})
          </span>
          {isJobActive && (
            <span className="flex items-center gap-1 font-mono text-2xs text-brand-400 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
              Live
            </span>
          )}
        </div>

        {/* Tab Switcher Segmentado ou Badge de Difusão */}
        {curves?.isDiffusion ? (
          <div className="flex items-center gap-1.5 rounded-lg border border-indigo-500/20 bg-indigo-500/10 px-2.5 py-1 font-mono text-2xs text-indigo-300 backdrop-blur-sm">
            <span className="size-1.5 rounded-full bg-indigo-400" />
            Curva de Loss Difusão
          </div>
        ) : (
          <div className="flex items-center rounded-lg border border-white/10 bg-black/40 backdrop-blur-sm p-0.5">
            <button
              type="button"
              onClick={() => setTab("all")}
              className={`rounded-md px-2.5 py-1 font-mono text-2xs transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
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
              className={`rounded-md px-2.5 py-1 font-mono text-2xs transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
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
              className={`rounded-md px-2.5 py-1 font-mono text-2xs transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
                tab === "map"
                  ? "bg-zinc-800 text-zinc-100 shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Precisão (mAP)
            </button>
          </div>
        )}
      </div>

      {/* Canvas Vetorial do Gráfico */}
      <ConvergenceChartCanvas
        curves={curves}
        metrics={metrics}
        tab={tab}
        hoverIndex={hoverIndex}
        setHoverIndex={setHoverIndex}
        totalEpochs={totalEpochs}
        viewBoxWidth={viewBoxWidth}
        viewBoxHeight={viewBoxHeight}
        padLeft={padLeft}
        padRight={padRight}
        padTop={padTop}
        padBottom={padBottom}
        graphWidth={graphWidth}
        graphHeight={graphHeight}
      />

      {/* Legenda Dinâmica com Valores do Ponto Focado / Mais Recente */}
      <ConvergenceMetricCards
        curves={curves}
        metrics={metrics}
        activeHover={activeHover}
        hoverIndex={hoverIndex}
        tab={tab}
      />
    </div>
  );
}
