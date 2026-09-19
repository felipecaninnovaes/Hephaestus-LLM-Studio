"use client";

import { useCallback, useRef, useState } from "react";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { uploadImages, type UploadResultItem } from "@/lib/images";

export interface UseDatasetUploadOptions {
  datasetId?: string;
  onSuccess?: () => Promise<void>;
}

export function useDatasetUpload({
  datasetId,
  onSuccess,
}: UseDatasetUploadOptions) {
  const fileRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);
  const [uploadCount, setUploadCount] = useState(0);
  const [uploadSent, setUploadSent] = useState(0);
  const [uploadBatchInfo, setUploadBatchInfo] = useState<{
    batchIndex: number;
    batchCount: number;
  } | null>(null);
  const uploadCancelledRef = useRef(false);
  const [auditModalOpen, setAuditModalOpen] = useState(false);
  const [lastUploadResults, setLastUploadResults] = useState<
    UploadResultItem[] | null
  >(null);

  const handleFiles = useCallback(
    async (files: FileList | File[] | null) => {
      if (
        !files ||
        (Array.isArray(files) ? files.length === 0 : files.length === 0) ||
        !datasetId ||
        uploading
      )
        return;

      const batch = Array.from(files);
      setUploading(true);
      setUploadCount(batch.length);
      setUploadSent(0);
      setUploadBatchInfo(null);
      uploadCancelledRef.current = false;

      try {
        const { items: results } = await uploadImages(datasetId, batch, {
          onProgress: (p) => {
            setUploadSent(p.sent);
            setUploadBatchInfo({
              batchIndex: p.batchIndex,
              batchCount: p.batchCount,
            });
          },
          isCancelled: () => uploadCancelledRef.current,
        });

        setLastUploadResults(results);
        const stored = results.filter(
          (r) => r.status === "stored" || r.status === "duplicate",
        );
        const problem = results.filter(
          (r) => r.status === "rejected" || r.status === "failed",
        );

        if (onSuccess) {
          await onSuccess();
        }

        const wasCancelled = uploadCancelledRef.current;
        if (wasCancelled) {
          const summary =
            problem.length > 0
              ? `${stored.length} imagens importadas de ${batch.length} antes do cancelamento, ${problem.length} rejeitadas.`
              : `${stored.length} imagens importadas de ${batch.length} antes do cancelamento.`;
          showToast(summary, "info");
        } else if (problem.length === 0) {
          showToast(
            `${stored.length} ${stored.length === 1 ? "imagem enviada." : "imagens enviadas."}`,
            "success",
          );
        } else {
          const examples = problem
            .slice(0, 3)
            .map((r) => `${r.filename} (${r.reason ?? r.status})`)
            .join(", ");
          const suffix =
            problem.length > 3 ? ` (+${problem.length - 3} mais)` : "";
          showToast(
            `${stored.length} enviadas, ${problem.length} rejeitadas: ${examples}${suffix}`,
            problem.length > stored.length ? "error" : "info",
          );
          setAuditModalOpen(true);
        }
      } catch (err) {
        const message =
          err instanceof ApiError && err.message
            ? err.message
            : "Falha ao enviar imagens.";
        showToast(message, "error");
      } finally {
        setUploading(false);
        setUploadCount(0);
        setUploadSent(0);
        setUploadBatchInfo(null);
        if (fileRef.current) fileRef.current.value = "";
      }
    },
    [datasetId, uploading, onSuccess],
  );

  const cancelUpload = useCallback(() => {
    uploadCancelledRef.current = true;
  }, []);

  return {
    fileRef,
    uploading,
    uploadCount,
    uploadSent,
    uploadBatchInfo,
    auditModalOpen,
    setAuditModalOpen,
    lastUploadResults,
    setLastUploadResults,
    handleFiles,
    cancelUpload,
  };
}
