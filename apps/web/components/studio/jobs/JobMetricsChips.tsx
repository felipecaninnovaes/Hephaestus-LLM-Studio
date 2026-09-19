"use client";

import { MetricSparkline } from "@/components/studio/ConvergenceChart";
import { imageProgressLabel } from "@/lib/jobCapabilities";
import type { JobMetrics as JobMetricsType } from "@/types/studio";

const SPARK_COLORS: Record<string, string> = {
  map50: "#34d399",
  map5095: "#2dd4bf",
  boxLoss: "#38bdf8",
  clsLoss: "#818cf8",
  dflLoss: "#fbbf24",
  loss: "#818cf8",
  lr: "#38bdf8",
  step: "#a1a1aa",
  epoch: "#a1a1aa",
};

interface JobMetricsChipsProps {
  metrics: JobMetricsType[];
  metricChipsType: "diffusion" | "yolo" | "progress" | false | null;
  step?: number | null;
  progress?: number | null;
}

export function JobMetricsChips({
  metrics,
  metricChipsType,
  step,
  progress,
}: JobMetricsChipsProps) {
  if (metricChipsType === "progress") {
    return (
      <div className="pt-3 border-t border-white/10">
        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 inline-flex items-center gap-2">
          <span className="text-3xs font-mono text-zinc-400 uppercase tracking-caps">
            Imagens processadas
          </span>
          <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
            {imageProgressLabel(step, progress)}
          </span>
        </div>
      </div>
    );
  }

  if (metrics.length === 0 || !metricChipsType) return null;

  const chips =
    metricChipsType === "diffusion"
      ? ([
          ["loss", "Diffusion Loss", false],
          ["lr", "Learning Rate", false],
          ["step", "Step", false],
          ["epoch", "Época", false],
        ] as const)
      : ([
          ["map50", "mAP@50", true],
          ["map5095", "mAP@50-95", true],
          ["boxLoss", "Box Loss", false],
          ["clsLoss", "Cls Loss", false],
          ["dflLoss", "Dfl Loss", false],
          ["epoch", "Epochs", false],
        ] as const);

  const last = metrics[metrics.length - 1];

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-300">
          Métricas (Epoch {last.epoch})
        </h3>
        <span className="font-mono text-2xs text-zinc-400">
          {metrics.length} checkpoint(s)
        </span>
      </div>
      <div
        className={`grid gap-2.5 ${
          metricChipsType === "diffusion"
            ? "grid-cols-2 sm:grid-cols-4"
            : "grid-cols-2 sm:grid-cols-3 lg:grid-cols-6"
        }`}
      >
        {chips.map(([key, label, isPercent]) => {
          const val = last[key as keyof JobMetricsType];
          const isPrimary = key === "map50" || key === "loss";
          const series = metrics.map(
            (m) => (m[key as keyof JobMetricsType] as number) ?? 0,
          );
          return (
            <div
              key={key}
              className={`rounded-xl border p-3 flex flex-col justify-between backdrop-blur-sm ${
                isPrimary
                  ? "border-brand-500/30 bg-brand-500/10"
                  : "border-white/10 bg-white/[0.02]"
              }`}
            >
              <div>
                <span
                  className={`block font-mono text-2xs font-medium tracking-caps uppercase ${
                    isPrimary ? "text-brand-300" : "text-zinc-400"
                  }`}
                >
                  {label}
                </span>
                <span className="block font-mono text-base font-semibold text-zinc-100 mt-1">
                  {typeof val === "number"
                    ? isPercent
                      ? `${(val * 100).toFixed(1)}%`
                      : key === "lr"
                        ? val.toExponential(2)
                        : key === "epoch" || key === "step"
                          ? val
                          : val.toFixed(4)
                    : "—"}
                </span>
              </div>
              <div className="mt-2 pt-2 border-t border-white/[0.04]">
                <MetricSparkline
                  data={series}
                  color={SPARK_COLORS[key] || "#34d399"}
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
