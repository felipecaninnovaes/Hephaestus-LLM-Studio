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

  const res = await fetch("/api/models/upload", {
    method: "POST",
    body: form,
    credentials: "same-origin",
  });

  if (!res.ok) {
    let code = "internal";
    let message = "";
    try {
      const envelope = (await res.json()) as {
        code?: string;
        message?: string;
      };
      if (typeof envelope.code === "string" && envelope.code)
        code = envelope.code;
      if (typeof envelope.message === "string") message = envelope.message;
    } catch {
      // corpo ilegível
    }
    const err = new Error(message || `Falha no upload: ${res.status}`);
    (err as unknown as { code: string }).code = code;
    (err as unknown as { status: number }).status = res.status;
    throw err;
  }

  return res.json() as Promise<Model>;
}

/** POST /api/models/download — download server-side por URL. Retorna 201 Model. */
export async function downloadModel(params: {
  url: string;
  engine: string;
  name?: string;
}): Promise<Model> {
  const res = await fetch("/api/models/download", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(params),
    credentials: "same-origin",
  });

  if (!res.ok) {
    let code = "internal";
    let message = "";
    try {
      const envelope = (await res.json()) as {
        code?: string;
        message?: string;
      };
      if (typeof envelope.code === "string" && envelope.code)
        code = envelope.code;
      if (typeof envelope.message === "string") message = envelope.message;
    } catch {
      // corpo ilegível
    }
    const err = new Error(message || `Falha no download: ${res.status}`);
    (err as unknown as { code: string }).code = code;
    (err as unknown as { status: number }).status = res.status;
    throw err;
  }

  return res.json() as Promise<Model>;
}
