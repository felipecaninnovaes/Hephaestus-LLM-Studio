import { apiFetch } from "@/lib/api";
import type {
  Job,
  JobArtifact,
  JobArtifactsResponse,
  JobListResponse,
  JobMetricsResponse,
  Telemetry,
  YoloAugment,
} from "@/types/studio";

/** POST /api/jobs/yolo — cria job de treino YOLO. Retorna 202. */
export function startYoloJob(params: {
  datasetId: string;
  model: string;
  epochs: number;
  batch: number;
  imgsz: number;
  lr0: number;
  optimizer: string;
  augment: YoloAugment;
  weights?: string | null;
  orchestratorId?: string | null;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  const { weights, orchestratorId, ...rest } = params;
  const body: Record<string, unknown> = { ...rest };
  if (weights) body.weights = weights;
  if (orchestratorId) body.orchestratorId = orchestratorId;
  return apiFetch("/api/jobs/yolo", {
    method: "POST",
    body,
  });
}

/** POST /api/jobs/diffusion — cria job de treino de difusão LoRA. Retorna 202. */
export function startDiffusionJob(params: {
  datasetId: string;
  baseModel: "sdxl" | "flux" | "sd15";
  triggerWord?: string;
  epochs?: number;
  batchSize?: number;
  learningRate?: number;
  rank?: number;
  alpha?: number;
  weights?: string | null;
  orchestratorId?: string | null;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  const { weights, orchestratorId, triggerWord, ...rest } = params;
  const body: Record<string, unknown> = { ...rest };
  if (triggerWord?.trim()) body.triggerWord = triggerWord.trim();
  if (weights) body.weights = weights;
  if (orchestratorId) body.orchestratorId = orchestratorId;
  return apiFetch("/api/jobs/diffusion", {
    method: "POST",
    body,
  });
}

/** GET /api/jobs — lista todos os jobs com status/progress. */
export function listJobs(): Promise<JobListResponse> {
  return apiFetch("/api/jobs");
}

/** GET /api/jobs/:id/metrics — série de métricas por epoch. */
export function getJobMetrics(jobId: string): Promise<JobMetricsResponse> {
  return apiFetch(`/api/jobs/${jobId}/metrics`);
}

/** GET /api/jobs/:id/artifacts — lista de artefatos do job. */
export function getJobArtifacts(jobId: string): Promise<JobArtifactsResponse> {
  return apiFetch(`/api/jobs/${jobId}/artifacts`);
}

/** GET /api/jobs/:id/artifacts/:artifactId/data — download de artefato via blob. */
export async function downloadArtifact(
  jobId: string,
  artifactId: string,
  filename: string,
): Promise<void> {
  const res = await fetch(`/api/jobs/${jobId}/artifacts/${artifactId}/data`, {
    credentials: "same-origin",
  });
  if (!res.ok) {
    throw new Error(`Falha ao baixar artefato: ${res.status}`);
  }
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}

/** POST /api/jobs/:id/abort — cancela um job. */
export function abortJob(jobId: string): Promise<void> {
  return apiFetch(`/api/jobs/${jobId}/abort`, { method: "POST" });
}

/** GET /api/telemetry — telemetria do nó (CPU/RAM/VRAM/GPUs). */
export function getTelemetry(): Promise<Telemetry> {
  return apiFetch("/api/telemetry");
}
