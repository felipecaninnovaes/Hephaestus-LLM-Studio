import { apiFetch } from "@/lib/api";
import type {
  AutotrackerApplyRequest,
  AutotrackerApplyResponse,
  AutotrackerPreviewResponse,
} from "@/types/studio";

/** POST /api/jobs/autotracker — cria job de AutoTracker. Retorna 202. */
export function startAutotrackerJob(params: {
  datasetId: string;
  model?: string;
  conf?: number;
  modelId?: string;
  orchestratorId?: string | null;
}): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  return apiFetch("/api/jobs/autotracker", {
    method: "POST",
    body: params,
  });
}

/** GET /api/jobs/:id/autotracker/preview — prévia de detecções e análise de classes do autotracker. */
export function getAutotrackerPreview(
  jobId: string,
): Promise<AutotrackerPreviewResponse> {
  return apiFetch<AutotrackerPreviewResponse>(
    `/api/jobs/${jobId}/autotracker/preview`,
  );
}

/** POST /api/jobs/:id/autotracker/apply — aplica boxes geradas ao dataset. */
export function applyAutotrackerBoxes(
  jobId: string,
  params?: AutotrackerApplyRequest,
): Promise<AutotrackerApplyResponse> {
  return apiFetch(`/api/jobs/${jobId}/autotracker/apply`, {
    method: "POST",
    body: params ?? {},
  });
}
