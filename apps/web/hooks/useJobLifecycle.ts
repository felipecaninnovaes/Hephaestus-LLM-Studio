"use client";

import { useState } from "react";
import { abortJob, deleteJob, downloadArtifact } from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { applyAutolabelCaptions } from "@/lib/autolabel";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/ui/Toast";
import {
  autolabelErrorMessage,
  autotrackerErrorMessage,
  type Job,
  type JobArtifact,
} from "@/types/studio";

export interface UseJobLifecycleOptions {
  /** Callback executado após qualquer mutação bem-sucedida (ex.: re-fetch de jobs) */
  onSuccess?: () => void | Promise<void>;
  /** Callback executado quando um job específico é excluído */
  onDeleted?: (jobId: string) => void;
  /** Callback para redirecionar para a página do dataset */
  onNavigateDataset?: (datasetId: string) => void;
}

/**
 * Hook canônico de mutação de ciclo de vida de jobs.
 * Unifica handlers de abort, delete, apply boxes, apply captions e download de artefatos
 * eliminando mais de 300 linhas de duplicação entre Jobs e ActionCenter.
 */
export function useJobLifecycle(options: UseJobLifecycleOptions = {}) {
  const { onSuccess, onDeleted, onNavigateDataset } = options;

  const [abortTarget, setAbortTarget] = useState<Job | null>(null);
  const [abortBusy, setAbortBusy] = useState(false);

  const [deleteTarget, setDeleteTarget] = useState<Job | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  const [applyBusy, setApplyBusy] = useState(false);
  const [applyOverwrite, setApplyOverwrite] = useState(false);

  async function handleAbort(): Promise<boolean> {
    if (!abortTarget) return false;
    setAbortBusy(true);
    try {
      await abortJob(abortTarget.id);
      showToast("Job cancelado com sucesso.", "success");
      setAbortTarget(null);
      await onSuccess?.();
      return true;
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "job_not_abortable" || err.status === 409)
      ) {
        showToast("Este job não pode mais ser cancelado.", "info");
        setAbortTarget(null);
        return false;
      }
      showToast("Falha ao cancelar job.", "error");
      return false;
    } finally {
      setAbortBusy(false);
    }
  }

  async function handleDeleteJob(): Promise<boolean> {
    if (!deleteTarget) return false;
    setDeleteBusy(true);
    const targetId = deleteTarget.id;
    try {
      const res = await deleteJob(targetId);
      showToast(
        `Job excluído · ${res.artifacts.length} artefatos, ${res.modelsDeleted} modelo(s) do catálogo${
          res.generationsPreserved > 0
            ? ` · ${res.generationsPreserved} geração(ões) da galeria preservadas`
            : ""
        }.`,
        "success",
      );
      setDeleteTarget(null);
      onDeleted?.(targetId);
      await onSuccess?.();
      return true;
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "job_not_terminal" || err.status === 409)
      ) {
        showToast(
          "Só jobs concluídos/falhos/cancelados podem ser excluídos.",
          "info",
        );
        setDeleteTarget(null);
        return false;
      }
      if (err instanceof ApiError && err.code === "not_found") {
        showToast("Job já havia sido removido.", "info");
        setDeleteTarget(null);
        onDeleted?.(targetId);
        await onSuccess?.();
        return false;
      }
      if (err instanceof ApiError && err.code === "queue_unavailable") {
        showToast("Manager indisponível, tente de novo.", "error");
        return false;
      }
      showToast("Falha ao excluir job.", "error");
      return false;
    } finally {
      setDeleteBusy(false);
    }
  }

  async function handleApplyBoxes(job: Job): Promise<boolean> {
    const datasetId = job.datasetId;
    setApplyBusy(true);
    try {
      const result = await applyAutotrackerBoxes(job.id, {
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} boxes aplicadas, ${result.skipped} ignoradas em ${result.images} imagem(ns).`,
        "success",
        datasetId && onNavigateDataset
          ? {
              label: "Abrir dataset",
              onClick: () => onNavigateDataset(datasetId),
            }
          : undefined,
      );
      if (typeof window !== "undefined" && datasetId) {
        window.dispatchEvent(
          new CustomEvent("hephaestus:dataset-updated", {
            detail: { datasetId },
          }),
        );
      }
      setApplyOverwrite(false);
      await onSuccess?.();
      return true;
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "job_not_done") {
          showToast("O job ainda não terminou — aguarde a conclusão.", "info");
          return false;
        }
        showToast(autotrackerErrorMessage(err.code), "error");
        return false;
      }
      showToast("Falha ao aplicar boxes ao dataset.", "error");
      return false;
    } finally {
      setApplyBusy(false);
    }
  }

  async function handleApplyCaptions(job: Job): Promise<boolean> {
    const datasetId = job.datasetId;
    setApplyBusy(true);
    try {
      const result = await applyAutolabelCaptions(job.id, {
        datasetId: datasetId ?? undefined,
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} legendas aplicadas, ${result.skipped} ignoradas em ${result.images} imagem(ns).`,
        "success",
        datasetId && onNavigateDataset
          ? {
              label: "Abrir dataset",
              onClick: () => onNavigateDataset(datasetId),
            }
          : undefined,
      );
      if (typeof window !== "undefined" && datasetId) {
        window.dispatchEvent(
          new CustomEvent("hephaestus:dataset-updated", {
            detail: { datasetId },
          }),
        );
      }
      setApplyOverwrite(false);
      await onSuccess?.();
      return true;
    } catch (err) {
      if (err instanceof ApiError) {
        showToast(autolabelErrorMessage(err.code), "error");
        return false;
      }
      showToast("Falha ao aplicar legendas ao dataset.", "error");
      return false;
    } finally {
      setApplyBusy(false);
    }
  }

  async function handleDownloadArtifact(jobId: string, art: JobArtifact) {
    try {
      const filename = art.path.split("/").pop() || "artefato.bin";
      await downloadArtifact(jobId, art.id, filename);
    } catch {
      showToast("Falha ao baixar artefato.", "error");
    }
  }

  return {
    abortTarget,
    setAbortTarget,
    abortBusy,
    handleAbort,
    deleteTarget,
    setDeleteTarget,
    deleteBusy,
    handleDeleteJob,
    applyBusy,
    applyOverwrite,
    setApplyOverwrite,
    handleApplyBoxes,
    handleApplyCaptions,
    handleDownloadArtifact,
  };
}
