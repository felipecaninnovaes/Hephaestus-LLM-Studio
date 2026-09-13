import { apiFetch } from "@/lib/api";
import type {
  AutolabelApplyRequest,
  AutolabelApplyResponse,
  AutolabelJobRequest,
  AutolabelPreviewResponse,
} from "@/types/studio";

/** POST /api/jobs/autolabel — cria job de AutoLabel. Retorna 202. */
export function startAutolabelJob(
  params: AutolabelJobRequest,
): Promise<{ jobId: string; status: string; queuePosition?: number }> {
  return apiFetch("/api/jobs/autolabel", {
    method: "POST",
    body: params,
  });
}

/** GET /api/jobs/:id/autolabel/preview — obtém prévia das legendas geradas para curadoria. */
export function getAutolabelPreview(
  jobId: string,
): Promise<AutolabelPreviewResponse> {
  return apiFetch(`/api/jobs/${jobId}/autolabel/preview`, {
    method: "GET",
  });
}

/** POST /api/jobs/:id/autolabel/apply — aplica legendas geradas (ou curadas) ao dataset. */
export function applyAutolabelCaptions(
  jobId: string,
  params?: AutolabelApplyRequest,
): Promise<AutolabelApplyResponse> {
  return apiFetch(`/api/jobs/${jobId}/autolabel/apply`, {
    method: "POST",
    body: params ?? {},
  });
}
