"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Button, ConfirmDialog, EmptyState, GlassCard, showToast } from "@/components/ui";
import { IconBox, IconDownload, IconPlus, IconTrash, IconUpload } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { deleteModel, listModels } from "@/lib/models";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import ModelUploadModal from "@/components/studio/ModelUploadModal";
import ModelDownloadModal from "@/components/studio/ModelDownloadModal";
import {
  modelSourceLabel,
  modelErrorMessage,
  type Model,
  type ModelSource,
} from "@/types/studio";

const SOURCE_BADGE_CLASSES: Record<ModelSource, string> = {
  train: "border-brand-500/35 bg-brand-500/10 text-brand-400",
  upload: "border-white/15 bg-white/[0.06] text-zinc-300",
  download: "border-white/15 bg-white/[0.06] text-zinc-300",
};

function engineBadge(engine: string) {
  switch (engine) {
    case "world":
      return { label: "YOLO-World", className: "border-sky-500/35 bg-sky-500/10 text-sky-400" };
    case "diffusion":
      return { label: "Difusão", className: "border-purple-500/35 bg-purple-500/10 text-purple-400" };
    case "clip":
      return { label: "CLIP", className: "border-[#34d399]/35 bg-[#34d399]/10 text-[#34d399]" };
    default:
      return { label: "YOLO", className: "border-zinc-700/60 bg-zinc-800/40 text-zinc-300" };
  }
}

function formatBadge(name: string): string | null {
  if (name.endsWith(".safetensors")) {
    return "safetensors";
  }
  if (name.endsWith(".pt")) {
    return ".pt";
  }
  return null;
}

export default function ModelsPage() {
  const router = useRouter();
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [downloadOpen, setDownloadOpen] = useState(false);
  const [deletingModel, setDeletingModel] = useState<Model | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

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
        err instanceof ApiError ? modelErrorMessage(err.code) : modelErrorMessage(""),
      );
    } finally {
      setLoading(false);
    }
  }, [router]);

  useEffect(() => {
    load();
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
        err instanceof ApiError ? modelErrorMessage(err.code) : "Falha ao remover modelo.",
        "error",
      );
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="mx-auto flex w-full max-w-7xl flex-col gap-4 px-4 sm:px-6 py-6">
      {/* Header */}
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="min-w-0">
          <div className="flex items-baseline gap-3">
            <h1
              className="font-display tracking-display truncate text-xl font-semibold text-zinc-100 lg:text-2xl"
              title="Modelos & Pesos"
            >
              Modelos & Pesos
            </h1>
            <span className="shrink-0 font-mono text-xs text-zinc-400">
              {models.length} {models.length === 1 ? "modelo" : "modelos"}
            </span>
          </div>
          <p className="mt-0.5 text-xs text-zinc-400">
            Checkpoints de treino, uploads e downloads. Envie pesos (.pt, .safetensors) ou baixe por URL.
          </p>
        </div>
        <div className="flex min-h-[44px] items-center gap-2">
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => setDownloadOpen(true)}
          >
            <IconDownload className="h-4 w-4 text-zinc-400" />
            Baixar por URL
          </Button>
          <Button
            type="button"
            variant="primary"
            size="md"
            onClick={() => setUploadOpen(true)}
          >
            <IconUpload className="h-4 w-4" />
            Enviar pesos
          </Button>
        </div>
      </div>

      {/* Conteúdo */}
      {loading ? (
        <p className="py-10 text-center font-mono text-xs text-zinc-400">
          Carregando modelos…
        </p>
      ) : error ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error}</p>
          <Button type="button" variant="secondary" size="md" onClick={load}>
            Tentar novamente
          </Button>
        </div>
      ) : models.length === 0 ? (
        <EmptyState
          icon={<IconBox className="h-6 w-6" />}
          title="Nenhum modelo ainda"
          description="Treine um job YOLO para gerar checkpoints ou envie pesos manualmente."
          actionLabel="Enviar pesos"
          onAction={() => setUploadOpen(true)}
          actionVariant="primary"
        />
      ) : (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-3 2xl:grid-cols-4">
          {models.map((m) => {
            const badge = engineBadge(m.engine);
            const format = formatBadge(m.name);
            return (
              <GlassCard key={m.id} interactive className="p-4">
                {/* Header do card */}
                <div className="flex items-start justify-between gap-2 mb-3">
                  <div className="min-w-0 flex-1">
                    <h3
                      className="font-mono text-sm font-semibold text-zinc-100 truncate"
                      title={m.name}
                    >
                      {m.name}
                    </h3>
                    <div className="mt-1 flex items-center gap-1.5 flex-wrap">
                      <span
                        className={`inline-flex items-center rounded px-1.5 py-0.5 font-mono text-[10px] font-medium uppercase tracking-[0.06em] border ${badge.className}`}
                      >
                        {badge.label}
                      </span>
                      {format && (
                        <span className="inline-flex items-center rounded border border-zinc-700/60 bg-zinc-800/60 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">
                          {format}
                        </span>
                      )}
                      {m.model && (
                        <span className="font-mono text-[11px] text-zinc-500">
                          · {m.model}
                        </span>
                      )}
                    </div>
                  </div>
                  <span
                    className={`shrink-0 rounded-full border px-2 py-0.5 font-mono text-[10px] uppercase tracking-[0.06em] ${SOURCE_BADGE_CLASSES[m.source]}`}
                    title={`Origem: ${modelSourceLabel(m.source)}`}
                  >
                    {modelSourceLabel(m.source)}
                  </span>
                </div>

                {/* Metadados */}
                <div className="space-y-1.5 text-[11px]">
                  <div className="flex items-center justify-between">
                    <span className="text-zinc-500">Tamanho</span>
                    <span className="font-mono text-zinc-300">
                      {formatBytes(m.bytes)}
                    </span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-zinc-500">Criado</span>
                    <span className="font-mono text-zinc-300">
                      {formatRelativeTime(m.createdAt)}
                    </span>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-zinc-500">MD5</span>
                    <span
                      className="font-mono text-zinc-400 truncate max-w-[140px]"
                      title={m.md5}
                    >
                      {m.md5}
                    </span>
                  </div>
                </div>

                {/* Ações */}
                <div className="mt-3 pt-3 border-t border-zinc-800/80 flex items-center gap-2">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className="flex-1"
                    onClick={() => handleDownload(m)}
                    disabled={!m.url}
                    title={
                      m.url
                        ? `Baixar ${m.name}`
                        : "Download indisponível — sem URL presigned"
                    }
                  >
                    <IconDownload className="h-3.5 w-3.5" />
                    Baixar
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="text-zinc-400 hover:text-red-400 hover:bg-red-500/10"
                    onClick={() => setDeletingModel(m)}
                    title={`Excluir ${m.name}`}
                    aria-label={`Excluir modelo ${m.name}`}
                  >
                    <IconTrash className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </GlassCard>
            );
          })}
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

      {/* Confirmação de Exclusão */}
      <ConfirmDialog
        open={deletingModel !== null}
        title="Excluir modelo"
        body={
          <span>
            Tem certeza que deseja excluir o modelo{" "}
            <strong className="font-mono text-zinc-100">{deletingModel?.name}</strong>?
            {deletingModel?.source !== "train"
              ? " O arquivo será removido do armazenamento de artefatos."
              : " O registro de modelo será desvinculado."}
          </span>
        }
        confirmLabel="Excluir"
        danger
        busy={deleteBusy}
        onConfirm={handleDeleteConfirm}
        onClose={() => {
          if (!deleteBusy) setDeletingModel(null);
        }}
      />
    </div>
  );
}
