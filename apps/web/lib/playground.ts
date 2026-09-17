import { apiFetch, ApiError } from "@/lib/api";
import type {
  DiffusionGenerateJobRequest,
  PredictJobRequest,
  PredictionsData,
  SubmitJobResponse,
} from "@/types/studio";

/**
 * POST /api/jobs/diffusion/generate — cria job de geração Text-to-Image por Difusão (ADR-0020/0023).
 * Suporta baseModel padrão OU customModelId (XOR), batch e multi-LoRA (v2).
 * Retorna 202 { jobId, status, queuePosition? }.
 */
export function startDiffusionGenerateJob(
  params: DiffusionGenerateJobRequest,
): Promise<SubmitJobResponse> {
  // Build body: only include baseModel or customModelId (XOR), never legacy weights/loraScale
  const body: Record<string, unknown> = {};
  if (params.customModelId) {
    body.customModelId = params.customModelId;
  } else if (params.baseModel) {
    body.baseModel = params.baseModel;
  }
  body.prompt = params.prompt;
  if (params.negativePrompt) body.negativePrompt = params.negativePrompt;
  if (params.width) body.width = params.width;
  if (params.height) body.height = params.height;
  if (params.steps) body.steps = params.steps;
  if (params.guidanceScale != null) body.guidanceScale = params.guidanceScale;
  if (params.seed != null) body.seed = params.seed;
  if (params.quantization) body.quantization = params.quantization;
  if (params.distilled != null) body.distilled = params.distilled;
  if (params.batchSize && params.batchSize > 1) body.batchSize = params.batchSize;
  if (params.loras && params.loras.length > 0) body.loras = params.loras;
  if (params.orchestratorId) body.orchestratorId = params.orchestratorId;

  return apiFetch("/api/jobs/diffusion/generate", {
    method: "POST",
    body,
  });
}

/**
 * Retorna a URL da imagem gerada por um job de difusão a partir dos artefatos.
 * Para batch (N>1), retorna a primeira imagem found; use getGeneratedBatchUrls para todas.
 */
export async function getGeneratedImageUrl(jobId: string): Promise<string | null> {
  try {
    const { items } = await apiFetch<{ items: { id: string; kind: string; path: string }[] }>(
      `/api/jobs/${jobId}/artifacts`,
    );
    const artifact = items.find((a) => a.kind === "generated" || a.path.endsWith("generated.png"));
    if (!artifact) return null;
    return `/api/jobs/${jobId}/artifacts/${artifact.id}/data`;
  } catch {
    return null;
  }
}

/**
 * Retorna URLs de todas as imagens e thumbs geradas por um job (batch support).
 * Cada item: { imageUrl, thumbUrl, seed (from meta), filename }.
 */
export async function getGeneratedBatchResults(
  jobId: string,
): Promise<{ imageUrl: string; thumbUrl: string | null; filename: string }[]> {
  const { items } = await apiFetch<{ items: { id: string; kind: string; path: string; md5: string; bytes: number }[] }>(
    `/api/jobs/${jobId}/artifacts`,
  );

  // Get all generated images (kind=generated or *.png)
  const generatedImages = items.filter(
    (a) => a.kind === "generated" || a.path.endsWith(".png"),
  );

  // If no batch-style artifacts, fallback to legacy single image
  if (generatedImages.length === 0) {
    return [];
  }

  return generatedImages.map((img) => ({
    imageUrl: `/api/jobs/${jobId}/artifacts/${img.id}/data`,
    thumbUrl: null, // thumbs will be resolved via generation_meta.json in the future
    filename: img.path.split("/").pop() || img.id,
  }));
}

/**
 * POST /api/jobs/predict — cria job de inferência YOLO.
 * Retorna 202 { jobId, status, queuePosition? }.
 */
export function startPredictJob(
  params: PredictJobRequest,
): Promise<SubmitJobResponse> {
  return apiFetch("/api/jobs/predict", {
    method: "POST",
    body: params,
  });
}

/**
 * Busca o predictions.json de um job predict concluído.
 * Usa o endpoint de artefatos existente: GET /api/jobs/:id/artifacts,
 * encontra o artefato kind='predictions', e baixa via proxy de data.
 * Erros usam ApiError para consistência com o resto da app.
 */
export async function getPredictions(jobId: string): Promise<PredictionsData> {
  // 1. Lista artefatos do job
  const { items } = await apiFetch<{ items: { id: string; kind: string }[] }>(
    `/api/jobs/${jobId}/artifacts`,
  );
  const artifact = items.find((a) => a.kind === "predictions");
  if (!artifact) {
    throw new ApiError(404, "not_found", "Artefato predictions não encontrado para este job.");
  }

  // 2. Baixa o conteúdo JSON do predictions
  const res = await fetch(
    `/api/jobs/${jobId}/artifacts/${artifact.id}/data`,
    { credentials: "same-origin" },
  );
  if (!res.ok) {
    let code = "internal";
    let message = "";
    try {
      const envelope = (await res.json()) as { code?: string; message?: string };
      if (typeof envelope.code === "string" && envelope.code) code = envelope.code;
      if (typeof envelope.message === "string") message = envelope.message;
    } catch {
      // Corpo ilegível
    }
    throw new ApiError(res.status, code, message || `Falha ao baixar predictions.json: ${res.status}`);
  }
  return res.json() as Promise<PredictionsData>;
}
