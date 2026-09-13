import { apiFetch, ApiError } from "@/lib/api";
import type {
  DiffusionGenerateJobRequest,
  PredictJobRequest,
  PredictionsData,
} from "@/types/studio";

/**
 * POST /api/jobs/diffusion/generate — cria job de geração Text-to-Image por Difusão (ADR-0020).
 * Retorna 202 { jobId, status, queuePosition? }.
 */
export function startDiffusionGenerateJob(
  params: DiffusionGenerateJobRequest,
): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  return apiFetch("/api/jobs/diffusion/generate", {
    method: "POST",
    body: params,
  });
}

/**
 * Retorna a URL da imagem gerada por um job de difusão a partir dos artefatos.
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
 * POST /api/jobs/predict — cria job de inferência YOLO.
 * Retorna 202 { jobId, status, queuePosition? }.
 */
export function startPredictJob(
  params: PredictJobRequest,
): Promise<{ jobId: string; status: string; queuePosition?: number }> {
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
