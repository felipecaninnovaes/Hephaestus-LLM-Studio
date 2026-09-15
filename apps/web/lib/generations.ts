import { apiFetch } from "@/lib/api";
import type {
  Generation,
  GenerationList,
  GenerationIdsRequest,
} from "@/types/studio";

interface ListGenerationsOpts {
  limit?: number;
  offset?: number;
  baseModel?: string;
  deleted?: boolean;
}

/**
 * GET /api/generations — lista gerações da galeria persistente.
 */
export function listGenerations(
  opts: ListGenerationsOpts = {},
): Promise<GenerationList> {
  const params = new URLSearchParams();
  if (opts.limit) params.set("limit", String(opts.limit));
  if (opts.offset) params.set("offset", String(opts.offset));
  if (opts.baseModel) params.set("baseModel", opts.baseModel);
  if (opts.deleted != null) params.set("deleted", String(opts.deleted));

  const qs = params.toString();
  return apiFetch(`/api/generations${qs ? `?${qs}` : ""}`);
}

/**
 * POST /api/generations/delete — soft-delete de gerações (≤100 ids).
 */
export function deleteGenerations(ids: string[]): Promise<void> {
  return apiFetch("/api/generations/delete", {
    method: "POST",
    body: { ids } as GenerationIdsRequest,
  });
}

/**
 * POST /api/generations/export — exporta gerações como zip (≤100 ids).
 * Retorna o blob do download.
 */
export async function exportGenerations(ids: string[]): Promise<void> {
  const res = await fetch("/api/generations/export", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ ids }),
    credentials: "same-origin",
  });

  if (!res.ok) {
    throw new Error(`Falha ao exportar gerações: ${res.status}`);
  }

  const disposition = res.headers.get("content-disposition");
  let filename = "geracoes.zip";
  if (disposition) {
    const match = disposition.match(/filename="?([^";]+)"?/i);
    if (match && match[1]) filename = match[1].trim();
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

/**
 * GET /api/generations/:id/data — retorna URL de dados (proxy) de uma geração.
 */
export function getGenerationDataUrl(id: string): string {
  return `/api/generations/${id}/data`;
}
