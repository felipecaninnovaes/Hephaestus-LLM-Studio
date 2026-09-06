import { apiFetch } from "@/lib/api";
import type {
  BoxInput,
  ImageDetail,
  ImagePage,
  PutBoxesResponse,
  UploadResultItem,
} from "@/types/studio";

export interface ListImagesOpts {
  limit?: number;
  offset?: number;
  split?: string;
  labeled?: boolean;
}

export function listImages(
  datasetId: string,
  opts?: ListImagesOpts,
): Promise<ImagePage> {
  const params = new URLSearchParams();
  if (opts?.limit !== undefined) params.set("limit", String(opts.limit));
  if (opts?.offset !== undefined) params.set("offset", String(opts.offset));
  if (opts?.split !== undefined) params.set("split", opts.split);
  if (opts?.labeled !== undefined) params.set("labeled", String(opts.labeled));
  const qs = params.toString();
  return apiFetch<ImagePage>(
    `/api/datasets/${datasetId}/images${qs ? `?${qs}` : ""}`,
  );
}

export function getImage(
  datasetId: string,
  imageId: string,
): Promise<ImageDetail> {
  return apiFetch<ImageDetail>(
    `/api/datasets/${datasetId}/images/${imageId}`,
  );
}

export function uploadImages(
  datasetId: string,
  files: File[],
): Promise<{ items: UploadResultItem[] }> {
  const form = new FormData();
  for (const file of files) form.append("files", file);
  return apiFetch<{ items: UploadResultItem[] }>(
    `/api/datasets/${datasetId}/upload`,
    { method: "POST", body: form },
  );
}

export function putBoxes(
  datasetId: string,
  imageId: string,
  boxes: BoxInput[],
): Promise<PutBoxesResponse> {
  return apiFetch<PutBoxesResponse>(
    `/api/datasets/${datasetId}/images/${imageId}/boxes`,
    { method: "PUT", body: { boxes } },
  );
}
