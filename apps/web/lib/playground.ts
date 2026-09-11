import { apiFetch } from "@/lib/api";
import type {
  PredictJobRequest,
  PredictionsData,
} from "@/types/studio";

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
 */
export async function getPredictions(jobId: string): Promise<PredictionsData> {
  // 1. Lista artefatos do job
  const { items } = await apiFetch<{ items: { id: string; kind: string }[] }>(
    `/api/jobs/${jobId}/artifacts`,
  );
  const artifact = items.find((a) => a.kind === "predictions");
  if (!artifact) {
    throw new Error("Artefato predictions não encontrado para este job.");
  }

  // 2. Baixa o conteúdo JSON do predictions
  const res = await fetch(
    `/api/jobs/${jobId}/artifacts/${artifact.id}/data`,
    { credentials: "same-origin" },
  );
  if (!res.ok) {
    throw new Error(`Falha ao baixar predictions.json: ${res.status}`);
  }
  return res.json() as Promise<PredictionsData>;
}
