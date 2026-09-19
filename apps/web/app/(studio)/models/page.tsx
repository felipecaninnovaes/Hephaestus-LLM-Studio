"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useState } from "react";
import ModelDownloadModal from "@/components/studio/ModelDownloadModal";
import {
  ModelCard,
  ModelHeader,
  ModelRenameDialog,
} from "@/components/studio/models";
import ModelUploadModal from "@/components/studio/ModelUploadModal";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { EmptyState } from "@/components/ui/EmptyState";
import { GlassCard } from "@/components/ui/GlassCard";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { deleteModel, listModels, updateModel } from "@/lib/models";
import { modelErrorMessage, type Model } from "@/types/studio";

export default function ModelsPage() {
  const router = useRouter();
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [downloadOpen, setDownloadOpen] = useState(false);
  const [deletingModel, setDeletingModel] = useState<Model | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [renamingModel, setRenamingModel] = useState<Model | null>(null);
  const [newModelName, setNewModelName] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listModels();
      setModels(data.items);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      setError(
        err instanceof ApiError
          ? modelErrorMessage(err.code)
          : modelErrorMessage(""),
      );
    } finally {
      setLoading(false);
    }
  }, [router]);

  useEffect(() => {
    void load();
  }, [load]);

  function handleDownload(model: Model) {
    if (!model.url) {
      showToast("Download indisponível — modelo sem URL presigned.", "info");
      return;
    }
    const a = document.createElement("a");
    a.href = model.url;
    a.download = model.name;
    a.rel = "noopener";
    document.body.appendChild(a);
    a.click();
    a.remove();
  }

  async function handleDeleteConfirm() {
    if (!deletingModel) return;
    setDeleteBusy(true);
    try {
      await deleteModel(deletingModel.id);
      setModels((prev) => prev.filter((m) => m.id !== deletingModel.id));
      showToast("Modelo removido com sucesso.", "success");
      setDeletingModel(null);
    } catch (err) {
      showToast(
        err instanceof ApiError
          ? modelErrorMessage(err.code)
          : "Falha ao remover modelo.",
        "error",
      );
    } finally {
      setDeleteBusy(false);
    }
  }

  async function handleRenameSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!renamingModel) return;
    const clean = newModelName.trim();
    if (!clean) return;
    setRenameBusy(true);
    try {
      const updated = await updateModel(renamingModel.id, clean);
      setModels((prev) =>
        prev.map((m) =>
          m.id === updated.id ? { ...m, name: updated.name } : m,
        ),
      );
      showToast("Modelo renomeado com sucesso.", "success");
      setRenamingModel(null);
    } catch (err) {
      showToast(
        err instanceof ApiError
          ? modelErrorMessage(err.code)
          : "Falha ao renomear modelo.",
        "error",
      );
    } finally {
      setRenameBusy(false);
    }
  }

  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-6 p-4 sm:p-6 lg:p-8">
      <ModelHeader
        onOpenUpload={() => setUploadOpen(true)}
        onOpenDownload={() => setDownloadOpen(true)}
      />

      {loading ? (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-3 2xl:grid-cols-4">
          {[1, 2, 3, 4].map((i) => (
            <GlassCard key={i} className="p-4 space-y-3 animate-pulse">
              <div className="h-4 w-3/4 rounded bg-white/10" />
              <div className="h-3 w-1/2 rounded bg-white/5" />
              <div className="h-8 rounded bg-white/5 mt-4" />
            </GlassCard>
          ))}
        </div>
      ) : error ? (
        <EmptyState
          title="Falha ao carregar modelos"
          description={error}
          actionLabel="Tentar novamente"
          onAction={() => void load()}
        />
      ) : models.length === 0 ? (
        <EmptyState
          title="Nenhum modelo ainda"
          description="Treine um job YOLO para gerar checkpoints ou envie pesos manualmente."
          actionLabel="Enviar pesos"
          onAction={() => setUploadOpen(true)}
          actionVariant="primary"
        />
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-3 2xl:grid-cols-4">
          {models.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              onDownload={handleDownload}
              onRename={(model) => {
                setRenamingModel(model);
                setNewModelName(model.name);
              }}
              onDelete={setDeletingModel}
            />
          ))}
        </div>
      )}

      {/* Modais */}
      <ModelUploadModal
        open={uploadOpen}
        onClose={() => setUploadOpen(false)}
        onUploaded={(m) =>
          setModels((prev) => [m, ...prev.filter((x) => x.id !== m.id)])
        }
      />
      <ModelDownloadModal
        open={downloadOpen}
        onClose={() => setDownloadOpen(false)}
        onDownloaded={(m) =>
          setModels((prev) => [m, ...prev.filter((x) => x.id !== m.id)])
        }
      />

      <ModelRenameDialog
        model={renamingModel}
        name={newModelName}
        setName={setNewModelName}
        busy={renameBusy}
        onClose={() => setRenamingModel(null)}
        onSubmit={(e) => void handleRenameSubmit(e)}
      />

      {/* Confirmação de exclusão */}
      <ConfirmDialog
        open={Boolean(deletingModel)}
        title="Excluir Modelo"
        body={
          deletingModel ? (
            <p>
              Tem certeza de que deseja excluir o modelo{" "}
              <strong className="text-zinc-100 font-mono">
                {deletingModel.name}
              </strong>
              ? O checkpoint será removido permanentemente do armazenamento.
            </p>
          ) : null
        }
        confirmLabel="Excluir"
        danger
        busy={deleteBusy}
        onConfirm={() => void handleDeleteConfirm()}
        onClose={() => setDeletingModel(null)}
      />
    </div>
  );
}
