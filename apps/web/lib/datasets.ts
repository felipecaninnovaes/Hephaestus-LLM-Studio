import { apiFetch } from "@/lib/api";
import type { CreateDatasetRequest, Dataset } from "@/types/studio";

export function listDatasets(): Promise<Dataset[]> {
  return apiFetch<Dataset[]>("/api/datasets");
}

export function createDataset(req: CreateDatasetRequest): Promise<Dataset> {
  return apiFetch<Dataset>("/api/datasets", {
    method: "POST",
    body: req,
  });
}

export function deleteDataset(id: string): Promise<void> {
  return apiFetch<void>(`/api/datasets/${id}`, { method: "DELETE" });
}
