import { apiFetch } from "@/lib/api";
import type {
  Model,
  ModelListResponse,
  ModelUploadInitRequest,
  ModelUploadInitResponse,
} from "@/types/studio";

/** GET /api/models — lista de modelos canônicos da tabela models. */
export function listModels(): Promise<ModelListResponse> {
  return apiFetch("/api/models");
}

/** POST /api/models/upload — upload multipart de pesos .pt/.safetensors. Retorna 201 Model. */
export async function uploadModel(params: {
  file: File;
  engine: string;
  name?: string;
  kind?: string;
  arch?: string;
}): Promise<Model> {
  const form = new FormData();
  form.append("file", params.file);
  form.append("engine", params.engine);
  if (params.name) form.append("name", params.name);
  if (params.kind) form.append("kind", params.kind);
  if (params.arch) form.append("arch", params.arch);

  return apiFetch<Model>("/api/models/upload", { method: "POST", body: form });
}

/** POST /api/models/download — download server-side por URL. Retorna 201 Model. */
export async function downloadModel(params: {
  url: string;
  engine: string;
  name?: string;
}): Promise<Model> {
  return apiFetch<Model>("/api/models/download", {
    method: "POST",
    body: params,
  });
}

/** DELETE /api/models/:id — remove modelo do catálogo e storage. Retorna 204. */
export async function deleteModel(id: string): Promise<void> {
  await apiFetch(`/api/models/${id}`, { method: "DELETE" });
}

/** PATCH /api/models/:id — atualiza o nome de um modelo (ADR-0022 D2). Retorna 200 Model. */
export async function updateModel(id: string, name: string): Promise<Model> {
  return apiFetch<Model>(`/api/models/${id}`, {
    method: "PATCH",
    body: { name },
  });
}

/* ── Upload chunked (contrato 901ebad) ─────────────────────────────── */
// Contorna o OOM do proxy Next (bufferiza multipart grande em RAM): partes
// vão como corpo cru octet-stream, nunca multipart. Sessões vivem em memória
// do principal — abortar via DELETE em falha/cancelamento (sem TTL no server).
export function initModelUpload(
  params: ModelUploadInitRequest,
): Promise<ModelUploadInitResponse> {
  return apiFetch<ModelUploadInitResponse>("/api/models/uploads/init", {
    method: "POST",
    body: params,
  });
}

/** PUT parte como corpo cru (SEM multipart). Retorna 204 (sem corpo). */
export async function uploadModelPart(
  uploadId: string,
  partNumber: number,
  blob: Blob,
): Promise<void> {
  await apiFetch(
    `/api/models/uploads/${uploadId}/part/${partNumber}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/octet-stream" },
      body: blob,
    },
  );
}

/** POST complete — monta, valida e executa o mesmo fluxo do multipart único. Retorna 201 Model. */
export function completeModelUpload(uploadId: string): Promise<Model> {
  return apiFetch<Model>(`/api/models/uploads/${uploadId}/complete`, {
    method: "POST",
  });
}

/** DELETE abort — idempotente (204 também se inexistente). Best-effort: chamar com .catch silencioso. */
export async function abortModelUpload(uploadId: string): Promise<void> {
  await apiFetch(`/api/models/uploads/${uploadId}`, { method: "DELETE" });
}

