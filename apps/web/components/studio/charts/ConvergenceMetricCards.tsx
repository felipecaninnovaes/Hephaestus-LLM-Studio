"use client";

import type { JobMetrics } from "@/types/jobs";
import type { ChartCurves, ChartTab } from "./chartMath";

export interface ConvergenceMetricCardsProps {
  curves: ChartCurves | null;
  metrics: JobMetrics[];
  activeHover: JobMetrics;
  hoverIndex: number | null;
  tab: ChartTab;
}

export function ConvergenceMetricCards({
  curves,
  metrics,
  activeHover,
  hoverIndex,
  tab,
}: ConvergenceMetricCardsProps) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 pt-1 border-t border-zinc-800/80 font-mono text-2xs">
      <div className="text-zinc-400 flex items-center gap-1.5">
        <span>Leitura:</span>
        <span className="text-zinc-200 font-semibold">
          Epoch {activeHover.epoch}
        </span>
        {hoverIndex != null && hoverIndex !== metrics.length - 1 && (
          <span className="text-brand-300">(Histórico)</span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        {curves?.isDiffusion ? (
          <>
            <div className="flex items-center gap-1.5">
              <span className="size-2 rounded-full bg-indigo-400" />
              <span className="text-zinc-400">Diffusion Loss:</span>
              <span className="font-semibold text-zinc-100">
                {activeHover.loss?.toFixed(4) ?? "—"}
              </span>
            </div>
            {activeHover.lr != null && (
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-sky-400" />
                <span className="text-zinc-400">LR:</span>
                <span className="font-semibold text-zinc-100">
                  {activeHover.lr.toExponential(2)}
                </span>
              </div>
            )}
            {activeHover.step != null && (
              <div className="flex items-center gap-1.5">
                <span className="size-2 rounded-full bg-zinc-500" />
                <span className="text-zinc-400">Step:</span>
                <span className="font-semibold text-zinc-100">
                  {activeHover.step}
                </span>
              </div>
            )}
          </>
        ) : (
          <>
            {(tab === "all" || tab === "map") && (
              <>
                <div className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-status-success" />
                  <span className="text-zinc-400">mAP@50:</span>
                  <span className="font-semibold text-zinc-100">
                    {((activeHover.map50 ?? 0) * 100).toFixed(1)}%
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-[#2dd4bf]" />
                  <span className="text-zinc-400">mAP@50-95:</span>
                  <span className="font-semibold text-zinc-100">
                    {((activeHover.map5095 ?? 0) * 100).toFixed(1)}%
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
                    {activeHover.boxLoss?.toFixed(4) ?? "—"}
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-[#818cf8]" />
                  <span className="text-zinc-400">Cls Loss:</span>
                  <span className="font-semibold text-zinc-100">
                    {activeHover.clsLoss?.toFixed(4) ?? "—"}
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  <span className="size-2 rounded-full bg-[#fbbf24]" />
                  <span className="text-zinc-400">DFL Loss:</span>
                  <span className="font-semibold text-zinc-100">
                    {activeHover.dflLoss?.toFixed(4) ?? "—"}
                  </span>
                </div>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
