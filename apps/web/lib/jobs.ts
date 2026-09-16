import { apiFetch } from "@/lib/api";
import type {
  Job,
  JobArtifact,
  JobArtifactsResponse,
  JobCleanupRequest,
  JobCleanupResponse,
  JobDeletedResponse,
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
  outputName?: string | null;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  const { weights, orchestratorId, outputName, ...rest } = params;
  const body: Record<string, unknown> = { ...rest };
  if (weights) body.weights = weights;
  if (orchestratorId) body.orchestratorId = orchestratorId;
  if (outputName?.trim()) body.outputName = outputName.trim();
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
  outputName?: string | null;
  samplePrompt?: string;
  sampleInterval?: number;
  sampleSeed?: number;
  resolution?: number;
  gradientAccumulationSteps?: number;
  optimizer?: "adamw8bit" | "adamw" | "prodigy";
  lrScheduler?: "cosine" | "linear" | "constant" | "constant_with_warmup";
  lrWarmupSteps?: number;
  mixedPrecision?: "fp16" | "bf16" | "no";
  quantization?: "none" | "4bit" | "8bit";
  enableBucket?: boolean;
  checkpointInterval?: number;
  epochOffset?: number;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  const {
    weights,
    orchestratorId,
    outputName,
    triggerWord,
    samplePrompt,
    sampleInterval,
    sampleSeed,
    resolution,
    gradientAccumulationSteps,
    optimizer,
    lrScheduler,
    lrWarmupSteps,
    mixedPrecision,
    quantization,
    enableBucket,
    checkpointInterval,
    epochOffset,
    ...rest
  } = params;
  const body: Record<string, unknown> = { ...rest };
  if (outputName?.trim()) body.outputName = outputName.trim();
  if (triggerWord?.trim()) body.triggerWord = triggerWord.trim();
  if (weights) body.weights = weights;
  if (orchestratorId) body.orchestratorId = orchestratorId;
  if (resolution) body.resolution = resolution;
  if (gradientAccumulationSteps != null) body.gradientAccumulationSteps = gradientAccumulationSteps;
  if (optimizer) body.optimizer = optimizer;
  if (lrScheduler) body.lrScheduler = lrScheduler;
  if (lrWarmupSteps != null) body.lrWarmupSteps = lrWarmupSteps;
  if (mixedPrecision) body.mixedPrecision = mixedPrecision;
  if (quantization) body.quantization = quantization;
  if (enableBucket != null) body.enableBucket = enableBucket;
  if (checkpointInterval != null) body.checkpointInterval = checkpointInterval;
  if (epochOffset != null) body.epochOffset = epochOffset;
  if (samplePrompt?.trim()) {
    body.samplePrompt = samplePrompt.trim();
    if (sampleInterval != null) body.sampleInterval = sampleInterval;
    if (sampleSeed != null) body.sampleSeed = sampleSeed;
  }
  return apiFetch("/api/jobs/diffusion", {
    method: "POST",
    body,
  });
}

/** GET /api/jobs — lista todos os jobs com status/progress. */
export function listJobs(): Promise<JobListResponse> {
  return apiFetch("/api/jobs");
}

/** GET /api/jobs/:id — detalhe de um job específico. */
export function getJob(jobId: string): Promise<Job> {
  return apiFetch(`/api/jobs/${jobId}`);
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
  const disposition = res.headers.get("content-disposition");
  let targetFilename = filename;
  if (disposition) {
    const match = disposition.match(/filename="?([^";]+)"?/i);
    if (match && match[1]) {
      targetFilename = match[1].trim();
    }
  }
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = targetFilename;
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

/** DELETE /api/jobs/:id — exclui um job terminal (AC-003). */
export function deleteJob(jobId: string): Promise<JobDeletedResponse> {
  return apiFetch(`/api/jobs/${jobId}`, { method: "DELETE" });
}

/** POST /api/jobs/cleanup — limpeza em lote de jobs terminais (AC-003). */
export function cleanupJobs(req: JobCleanupRequest): Promise<JobCleanupResponse> {
  return apiFetch("/api/jobs/cleanup", { method: "POST", body: req });
}

/** GET /api/telemetry — telemetria do nó (CPU/RAM/VRAM/GPUs). */
export function getTelemetry(): Promise<Telemetry> {
  return apiFetch("/api/telemetry");
}
