"use client";

import { useJobTelemetry } from "@/hooks/useJobTelemetry";
import { jobCapabilities } from "@/lib/jobCapabilities";
import { formatDurationMs } from "@/lib/format";
import type { Job } from "@/types/studio";

/** Treino em execução (difusão/YOLO). Exclui geração e predição. */
export function isRunningTrainingJob(
  job: Pick<Job, "kind" | "engine" | "mode" | "status">,
): boolean {
  if (job.status !== "running") return false;
  if ((job.kind as string) === "diffusion_generate") return false;
  if (job.kind === "diffusion" && job.mode === "generate") return false;
  return jobCapabilities(job).convergenceChart;
}

/**
 * Rótulo "≈ {tempo} restantes" a partir do ETA do servidor; `null` quando o
 * servidor não informou ETA (sem fallback no cliente aqui).
 */
export function trainingEtaLabel(
  etaFormatted: string | null | undefined,
  etaSeconds: number | null | undefined,
): string | null {
  const formatted = etaFormatted?.trim();
  if (formatted) return `≈ ${formatted.replace(/^eta\s*~?\s*/i, "")} restantes`;
  if (typeof etaSeconds === "number" && Number.isFinite(etaSeconds) && etaSeconds > 0) {
    return `≈ ${formatDurationMs(etaSeconds * 1000)} restantes`;
  }
  return null;
}

/** ETA ao vivo de um treino em execução; só é montado para esse caso (1 SSE por item). */
export function ActionCenterJobEta({ jobId }: { jobId: string }) {
  const { etaSeconds, etaFormatted, isFinished } = useJobTelemetry(jobId);
  const label = isFinished ? null : trainingEtaLabel(etaFormatted, etaSeconds);
  if (!label) return null;
  return (
    <span
      className="shrink-0 text-brand-400"
      data-testid="action-center-job-eta"
    >
      {label}
    </span>
  );
}
