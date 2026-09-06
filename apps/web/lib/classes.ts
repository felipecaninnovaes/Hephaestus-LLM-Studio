import { apiFetch } from "@/lib/api";
import type {
  PutClassesResponse,
  PutClassInput,
} from "@/types/studio";

/** Mesma regex do backend (`normalize_classes`): letras, números e _ (máx 64). */
export const CLASS_RE = /^[A-Za-z0-9_][A-Za-z0-9_]{0,63}$/;
export const MAX_CLASSES = 200;

export function putClasses(
  datasetId: string,
  classes: PutClassInput[],
): Promise<PutClassesResponse> {
  return apiFetch<PutClassesResponse>(`/api/datasets/${datasetId}/classes`, {
    method: "PUT",
    body: { classes },
  });
}
