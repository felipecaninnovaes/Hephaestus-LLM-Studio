import { apiFetch } from "@/lib/api";
import type { SearchResponse, SearchStatus } from "@/types/studio";

export interface TextSearchOpts {
  k?: number;
  classId?: string;
  split?: string;
  signal?: AbortSignal;
}

export function searchDataset(
  datasetId: string,
  q: string,
  opts?: TextSearchOpts,
): Promise<SearchResponse> {
  const params = new URLSearchParams();
  params.set("q", q);
  if (opts?.k !== undefined) params.set("k", String(opts.k));
  if (opts?.classId) params.set("classId", opts.classId);
  if (opts?.split) params.set("split", opts.split);
  return apiFetch<SearchResponse>(
    `/api/datasets/${datasetId}/search?${params.toString()}`,
    { signal: opts?.signal },
  );
}

export interface ByImageSearchOpts {
  k?: number;
  threshold?: number;
  signal?: AbortSignal;
}

export function searchByImage(
  datasetId: string,
  imageId: string,
  opts?: ByImageSearchOpts,
): Promise<SearchResponse> {
  return apiFetch<SearchResponse>(
    `/api/datasets/${datasetId}/search/by-image`,
    {
      method: "POST",
      body: {
        imageId,
        ...(opts?.k !== undefined ? { k: opts.k } : {}),
        ...(opts?.threshold !== undefined ? { threshold: opts.threshold } : {}),
      },
      signal: opts?.signal,
    },
  );
}

export function getSearchStatus(
  datasetId: string,
  signal?: AbortSignal,
): Promise<SearchStatus> {
  return apiFetch<SearchStatus>(
    `/api/datasets/${datasetId}/search/status`,
    { signal },
  );
}

export function triggerSearchIndex(
  datasetId: string,
): Promise<{ status: "indexing" | "not_indexed" }> {
  // 202 pode vir sem corpo dependendo do caminho — garante objeto.
  return apiFetch<{ status: "indexing" | "not_indexed" }>(
    `/api/datasets/${datasetId}/search/index`,
    { method: "POST" },
  ).then((res) => res ?? { status: "indexing" as const });
}
