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
  deleted?: boolean;
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
  if (opts?.deleted !== undefined) params.set("deleted", String(opts.deleted));
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

// ---------------------------------------------------------------------------
// Chunked upload with progress, cancellation, and per-batch error handling
// ---------------------------------------------------------------------------

const BATCH_MAX_BYTES = 24 * 1024 * 1024; // 24 MiB per batch
const BATCH_MAX_FILES = 20; // max files per batch
const PER_FILE_MAX_BYTES = 200 * 1024 * 1024; // 200 MiB per-file cap

export interface UploadImagesOpts {
  onProgress?: (p: {
    sent: number;
    total: number;
    batchIndex: number;
    batchCount: number;
  }) => void;
  isCancelled?: () => boolean;
}

/**
 * Split files into batches respecting weight (~24 MiB) and count (max 20)
 * limits. Files already validated (>200 MiB) are excluded beforehand.
 */
function buildBatches(files: File[]): File[][] {
  const batches: File[][] = [];
  let currentBatch: File[] = [];
  let currentBytes = 0;

  for (const file of files) {
    if (currentBatch.length >= BATCH_MAX_FILES || currentBytes + file.size > BATCH_MAX_BYTES) {
      if (currentBatch.length > 0) batches.push(currentBatch);
      currentBatch = [];
      currentBytes = 0;
    }
    currentBatch.push(file);
    currentBytes += file.size;
  }
  if (currentBatch.length > 0) batches.push(currentBatch);
  return batches;
}

/**
 * Upload images with chunking, progress callbacks, cancellation support,
 * and honest batch-level error handling.
 *
 * Backward-compatible: calling without `opts` works exactly as before
 * (chunked but no progress/cancel).
 */
export async function uploadImages(
  datasetId: string,
  files: File[],
  opts?: UploadImagesOpts,
): Promise<{ items: UploadResultItem[] }> {
  const { onProgress, isCancelled } = opts ?? {};

  // 1. Pre-validate: files > 200 MiB → rejected immediately (no network)
  const oversized: UploadResultItem[] = [];
  const validFiles: File[] = [];
  for (const file of files) {
    if (file.size > PER_FILE_MAX_BYTES) {
      oversized.push({
        imageId: null,
        filename: file.name,
        status: "rejected",
        reason: "too_large",
        bytes: file.size,
        width: null,
        height: null,
      });
    } else {
      validFiles.push(file);
    }
  }

  // 2. Build batches from valid files
  const batches = buildBatches(validFiles);
  const batchCount = batches.length;
  const allItems: UploadResultItem[] = [...oversized];
  let sentCount = oversized.length; // pre-validated count
  const totalFiles = files.length;

  // 3. Send batches sequentially
  for (let i = 0; i < batches.length; i++) {
    // Check cancellation between batches
    if (isCancelled?.()) break;

    const batch = batches[i];
    const form = new FormData();
    for (const file of batch) form.append("files", file);

    try {
      const result = await apiFetch<{ items: UploadResultItem[] }>(
        `/api/datasets/${datasetId}/upload`,
        { method: "POST", body: form },
      );
      allItems.push(...result.items);
      sentCount += batch.length;
    } catch (err: unknown) {
      // Determine if this is a recoverable error or a hard stop (413 envelope)
      const isEnvelopeLimit =
        err instanceof Object && "status" in err && (err as { status: number }).status === 413;

      // Mark every file in this failed batch as rejected
      for (const file of batch) {
        allItems.push({
          imageId: null,
          filename: file.name,
          status: "rejected",
          reason: isEnvelopeLimit ? "too_large" : "storage_error",
          bytes: file.size,
          width: null,
          height: null,
        });
      }
      sentCount += batch.length;

      // 413 = envelope too large — subsequent batches would also fail → stop
      if (isEnvelopeLimit) break;

      // Other errors (network, 500): continue with remaining batches
    }

    // Report progress after each batch completes
    onProgress?.({
      sent: sentCount,
      total: totalFiles,
      batchIndex: i + 1,
      batchCount,
    });
  }

  return { items: allItems };
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

export function softDeleteImage(
  datasetId: string,
  imageId: string,
): Promise<void> {
  return apiFetch<void>(`/api/datasets/${datasetId}/images/${imageId}`, {
    method: "DELETE",
  });
}

export interface RestoreImageResult {
  filename?: string;
}

export function restoreImage(
  datasetId: string,
  imageId: string,
): Promise<RestoreImageResult> {
  // 204 (sem conflito) não tem corpo — o apiFetch resolve `undefined`.
  return apiFetch<RestoreImageResult>(
    `/api/datasets/${datasetId}/images/${imageId}/restore`,
    { method: "POST" },
  ).then((res) => res ?? {});
}

export function purgeTrash(datasetId: string): Promise<void> {
  return apiFetch<void>(`/api/datasets/${datasetId}/trash`, {
    method: "DELETE",
  });
}
