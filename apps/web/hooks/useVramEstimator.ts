"use client";

import { useMemo } from "react";

export type OomRisk = "safe" | "warning" | "danger";

/**
 * Heurística pura para cálculo de risco de estouro de memória (CUDA OOM).
 */
export function calculateOomRisk(
  estimatedVram: number,
  nodeVramTotalGb: number | null,
  warningThreshold = 0.85,
): OomRisk {
  if (nodeVramTotalGb == null) return "safe";
  if (estimatedVram > nodeVramTotalGb) return "danger";
  if (estimatedVram > nodeVramTotalGb * warningThreshold) return "warning";
  return "safe";
}

/**
 * Hook para avaliação preditiva de risco de memória de GPU durante parametrização de treino.
 */
export function useVramEstimator(
  estimatedVram: number,
  nodeVramTotalGb: number | null,
  warningThreshold = 0.85,
) {
  const oomRisk = useMemo<OomRisk>(
    () => calculateOomRisk(estimatedVram, nodeVramTotalGb, warningThreshold),
    [estimatedVram, nodeVramTotalGb, warningThreshold],
  );

  return {
    oomRisk,
    isSafe: oomRisk === "safe",
    isWarning: oomRisk === "warning",
    isDanger: oomRisk === "danger",
  };
}
