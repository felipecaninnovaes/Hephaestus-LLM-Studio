import { apiFetch } from "@/lib/api";
import type { Dataset } from "@/types/studio";

export function listDatasets(): Promise<Dataset[]> {
  return apiFetch<Dataset[]>("/api/datasets");
}
