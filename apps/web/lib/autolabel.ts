import { apiFetch } from "@/lib/api";
import type {
  AutolabelApplyRequest,
  AutolabelApplyResponse,
  AutolabelJobRequest,
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

/** POST /api/jobs/:id/autolabel/apply — aplica legendas geradas ao dataset. */
export function applyAutolabelCaptions(
  jobId: string,
  params?: AutolabelApplyRequest,
): Promise<AutolabelApplyResponse> {
  return apiFetch(`/api/jobs/${jobId}/autolabel/apply`, {
    method: "POST",
    body: params ?? {},
  });
}
