import { apiFetch } from "@/lib/api";
import type {
  BoxInput,
  CaptionData,
  ImageDetail,
  ImagePage,
  PutBoxesResponse,
  UploadResultItem,
} from "@/types/studio";

export type { UploadResultItem };

export interface ListImagesOpts {
  limit?: number;
  offset?: number;
  split?: string;
  labeled?: boolean;
  deleted?: boolean;
  classId?: string;
  tag?: string;
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
  if (opts?.classId !== undefined && opts.classId) params.set("classId", opts.classId);
  if (opts?.tag !== undefined && opts.tag.trim()) params.set("tag", opts.tag.trim());
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

const BATCH_MAX_BYTES = 8 * 1024 * 1024; // 8 MiB per batch
const BATCH_MAX_FILES = 25; // max files per batch
const PER_FILE_MAX_BYTES = 200 * 1024 * 1024; // 200 MiB per-file cap
const CONCURRENCY = 2; // max simultaneous batch requests in flight

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
 * Split files into batches respecting weight (~24 MiB) and count (max 40)
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
 * concurrency of up to 2 batches in flight, and honest batch-level error handling.
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
  let sentCount = 0;
  const totalFiles = validFiles.length;

  // 3. Send batches with concurrency limit (max 2 in flight)
  let nextBatchIdx = 0;
  let hasHardStop = false;
  let completedBatches = 0;

  async function worker() {
    while (nextBatchIdx < batches.length) {
      if (isCancelled?.() || hasHardStop) break;
      const i = nextBatchIdx++;
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
        const stored = result.items.filter((it) => it.status === "stored").length;
        const dups = result.items.filter((it) => it.status === "duplicate").length;
        const rejected = result.items.filter((it) => it.status === "rejected").length;
        const failed = result.items.filter((it) => it.status === "failed").length;
        console.info(
          `[upload] Lote ${i + 1}/${batchCount} enviado (${batch.length} arquivos): ` +
            `${stored} armazenados, ${dups} duplicados, ${rejected} rejeitados, ${failed} falhas`,
        );
        if (rejected > 0 || failed > 0) {
          const problems = result.items.filter((it) => it.status === "rejected" || it.status === "failed");
          console.warn(`[upload] Detalhes dos problemas no lote ${i + 1}:`, problems);
        }
      } catch (err: unknown) {
        console.error(`[upload] Falha crítica no lote ${i + 1}/${batchCount}:`, err);
        const isEnvelopeLimit =
          err instanceof Object && "status" in err && (err as { status: number }).status === 413;

        for (const file of batch) {
          allItems.push({
            imageId: null,
            filename: file.name,
            status: "failed",
            reason: isEnvelopeLimit ? "envelope_limit" : "storage_error",
            bytes: file.size,
            width: null,
            height: null,
          });
        }
        sentCount += batch.length;

        if (isEnvelopeLimit) {
          hasHardStop = true;
          break;
        }
      }

      completedBatches++;
      onProgress?.({
        sent: sentCount,
        total: totalFiles,
        batchIndex: completedBatches,
        batchCount,
      });
    }
  }

  const workerCount = Math.min(CONCURRENCY, batches.length);
  if (workerCount > 0) {
    const workers = Array.from({ length: workerCount }, () => worker());
    await Promise.all(workers);
  }

  // If stopped early by envelope limit, mark remaining unsent batches
  if (hasHardStop) {
    for (let r = nextBatchIdx; r < batches.length; r++) {
      for (const file of batches[r]) {
        allItems.push({
          imageId: null,
          filename: file.name,
          status: "failed",
          reason: "envelope_limit",
          bytes: file.size,
          width: null,
          height: null,
        });
      }
    }
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

export function putCaption(
  datasetId: string,
  imageId: string,
  data: { text: string; origin?: string },
): Promise<CaptionData> {
  return apiFetch<CaptionData>(
    `/api/datasets/${datasetId}/images/${imageId}/caption`,
    {
      method: "PUT",
      body: data,
    },
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
