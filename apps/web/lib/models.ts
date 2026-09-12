import { apiFetch } from "@/lib/api";
import type { Model, ModelListResponse } from "@/types/studio";

/** GET /api/models — lista de modelos canônicos da tabela models. */
export function listModels(): Promise<ModelListResponse> {
  return apiFetch("/api/models");
}

/** POST /api/models/upload — upload multipart de pesos .pt. Retorna 201 Model. */
export async function uploadModel(params: {
  file: File;
  engine: string;
  name?: string;
}): Promise<Model> {
  const form = new FormData();
  form.append("file", params.file);
  form.append("engine", params.engine);
  if (params.name) form.append("name", params.name);

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

