import { apiFetch } from "@/lib/api";
import type { AutotrackerApplyResponse } from "@/types/studio";

/** POST /api/jobs/autotracker — cria job de AutoTracker. Retorna 202. */
export function startAutotrackerJob(params: {
  datasetId: string;
  model?: string;
  conf?: number;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  return apiFetch("/api/jobs/autotracker", {
    method: "POST",
    body: params,
  });
}

/** POST /api/jobs/:id/autotracker/apply — aplica boxes geradas ao dataset. */
export function applyAutotrackerBoxes(
  jobId: string,
  params?: { overwrite?: boolean },
): Promise<AutotrackerApplyResponse> {
  return apiFetch(`/api/jobs/${jobId}/autotracker/apply`, {
    method: "POST",
    body: params ?? {},
  });
}
