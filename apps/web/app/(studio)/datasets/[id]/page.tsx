"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { showToast } from "@/components/studio/Toast";
import {
  IconDatabase,
  IconDownload,
  IconFolder,
  IconPlay,
  IconPlus,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { getDataset } from "@/lib/datasets";
import { listImages, uploadImages } from "@/lib/images";
import { formatBytes } from "@/lib/format";
import type { Dataset, ImageItem } from "@/types/studio";

const PAGE_LIMIT = 50;

export default function DatasetGalleryPage() {
  const params = useParams<{ id: string }>();
  const datasetId = params.id;
  const router = useRouter();
  const fileRef = useRef<HTMLInputElement>(null);

  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [items, setItems] = useState<ImageItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadCount, setUploadCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    async (id: string) => {
      setLoading(true);
      setError(null);
      try {
        const [ds, page] = await Promise.all([
          getDataset(id),
          listImages(id, { limit: PAGE_LIMIT, offset: 0 }),
        ]);
        setDataset(ds);
        setItems(page.items);
        setTotal(page.total);
      } catch (err) {
        if (
          err instanceof ApiError &&
          (err.code === "unauthorized" || err.status === 401)
        ) {
          router.replace("/login");
          return;
        }
        if (err instanceof ApiError && err.status === 404) {
          setError("Dataset não encontrado.");
        } else {
          setError("Falha ao carregar a galeria.");
        }
      } finally {
        setLoading(false);
      }
    },
    [router],
  );

  useEffect(() => {
    if (datasetId) load(datasetId);
  }, [datasetId, load]);

  async function loadMore() {
    if (!datasetId || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await listImages(datasetId, {
        limit: PAGE_LIMIT,
        offset: items.length,
      });
      setItems((prev) => [...prev, ...page.items]);
      setTotal(page.total);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast("Falha ao carregar mais imagens.", "error");
    } finally {
      setLoadingMore(false);
    }
  }

  async function handleFiles(files: FileList | null) {
    if (!files || files.length === 0 || !datasetId || uploading) return;
    const batch = Array.from(files);
    setUploading(true);
    setUploadCount(batch.length);
    try {
      const { items: results } = await uploadImages(datasetId, batch);
      const stored = results.filter((r) => r.status === "stored");
      const problem = results.filter((r) => r.status !== "stored");
      await load(datasetId);
      if (problem.length === 0) {
        showToast(
          `${stored.length} ${stored.length === 1 ? "imagem enviada." : "imagens enviadas."}`,
          "success",
        );
      } else {
        const examples = problem
          .slice(0, 2)
          .map((r) => `${r.filename} (${r.reason ?? r.status})`)
          .join(", ");
        showToast(
          `${problem.length} rejeitadas: ${examples}`,
          "info",
        );
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
      if (fileRef.current) fileRef.current.value = "";
    }
  }

  function handleTileClick(item: ImageItem) {
    if (!dataset) return;
    if (dataset.category === "yolo") {
      router.push(`/datasets/${datasetId}/annotate/${item.id}`);
    } else {
      showToast(
        "Revisão de caption chega no AutoLabel (fatia futura).",
        "info",
      );
    }
  }

  if (loading) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <p className="py-10 text-center text-sm text-zinc-500">Carregando…</p>
      </div>
    );
  }

  if (error || !dataset) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <button
          type="button"
          onClick={() => router.push("/datasets")}
          className="w-fit rounded-lg px-2 py-1 text-xs font-medium text-zinc-400 transition-colors hover:bg-zinc-900/60 hover:text-zinc-200"
        >
          ← Datasets
        </button>
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error ?? "Dataset não encontrado."}</p>
          <button
            type="button"
            onClick={() => datasetId && load(datasetId)}
            className="rounded-lg border border-zinc-700/80 bg-zinc-900/60 px-4 py-2 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-800"
          >
            Tentar novamente
          </button>
        </div>
      </div>
    );
  }

  const reviewer = dataset.category === "yolo" ? "AutoTracker" : "AutoLabel";

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
      <div className="flex flex-col justify-between gap-4 md:flex-row md:items-center">
        <div className="flex items-center space-x-3">
          <button
            type="button"
            onClick={() => router.push("/datasets")}
            className="rounded-lg border border-zinc-800 bg-zinc-900 p-2 text-zinc-300 transition-colors hover:bg-zinc-800 hover:text-white"
            title="Voltar para a lista de datasets"
            aria-label="Voltar para a lista de datasets"
          >
            <svg
              className="h-4 w-4"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.7"
              viewBox="0 0 24 24"
            >
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </button>
          <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-emerald-500/30 bg-emerald-500/10 text-emerald-400">
            <IconDatabase className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-base font-bold tracking-tight text-white">
              {dataset.title}
            </h2>
            <p className="font-mono text-xs text-zinc-400">
              {dataset.type} · {dataset.imagesCount.toLocaleString()} imagens ·{" "}
              {formatBytes(dataset.sizeBytes)} · {dataset.source ?? "—"}
            </p>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            disabled
            title="Preparo assistido chega numa fatia futura."
            className="flex items-center space-x-1.5 rounded-lg border border-zinc-700/80 bg-zinc-900 px-3 py-2 text-xs font-medium text-zinc-200 opacity-60"
          >
            <IconSparkles className="h-4 w-4" />
            <span>AutoLabel</span>
          </button>
          <button
            type="button"
            disabled
            title="Geração automática de boxes chega na fatia 4."
            className="flex items-center space-x-1.5 rounded-lg border border-zinc-700/80 bg-zinc-900 px-3 py-2 text-xs font-medium text-zinc-200 opacity-60"
          >
            <IconTarget className="h-4 w-4" />
            <span>AutoTracker</span>
          </button>
          <button
            type="button"
            disabled
            title="Backup estruturado chega na fatia 3e."
            className="flex items-center space-x-1.5 rounded-lg border border-zinc-700/80 bg-zinc-900 px-3 py-2 text-xs font-medium text-zinc-200 opacity-60"
          >
            <IconDownload className="h-4 w-4" />
            <span>Exportar</span>
          </button>
          <button
            type="button"
            disabled
            title="Treino chega na fatia 4."
            className="flex items-center space-x-1.5 rounded-lg bg-emerald-500 px-4 py-2 text-xs font-semibold text-zinc-950 opacity-60 shadow-lg shadow-emerald-500/20"
          >
            <IconPlay className="h-4 w-4" />
            <span>Treinar este Dataset</span>
          </button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 rounded-2xl border border-zinc-800/80 bg-zinc-950/60 px-4 py-2.5 font-mono text-[11px] text-zinc-400">
        <span className="font-semibold text-zinc-300">
          {dataset.imagesCount.toLocaleString()} amostras
        </span>
        <span className="h-3 w-px bg-zinc-700"></span>
        <span>
          <span className="text-emerald-400">
            {dataset.labeledCount.toLocaleString()}
          </span>{" "}
          rotuladas por {reviewer}
        </span>
        <span className="h-3 w-px bg-zinc-700"></span>
        <span>Formato: {dataset.format}</span>
        <span className="h-3 w-px bg-zinc-700"></span>
        <span>
          Backup: exportar/importar mantém JSON/YAML de config + anotações
        </span>
      </div>

      {dataset.imagesCount === 0 ? (
        <div className="glass-card rounded-2xl border border-zinc-800 px-4 py-14 text-center">
          <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
            <IconFolder className="h-5 w-5" />
          </div>
          <h3 className="text-sm font-semibold text-zinc-200">Galeria vazia</h3>
          <p className="mx-auto mt-1 max-w-sm text-xs text-zinc-400">
            Este dataset ainda não tem amostras. Envie imagens pelo botão
            abaixo; o upload é gerido pelo backend.
          </p>
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            disabled={uploading}
            className="mt-4 rounded-lg bg-zinc-800 px-3 py-1.5 text-xs text-zinc-200 transition-colors hover:bg-zinc-700 disabled:opacity-60"
          >
            {uploading ? `Enviando ${uploadCount} arquivo(s)…` : "Enviar amostras"}
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
          {items.map((item) => (
            <div
              key={item.id}
              onClick={() => handleTileClick(item)}
              className="group relative h-36 cursor-pointer overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/90 transition-all hover:border-emerald-500/60"
            >
              <img
                src={item.url}
                alt={item.filename}
                loading="lazy"
                className="absolute inset-0 h-full w-full object-cover"
              />
              <div className="absolute inset-0 bg-[radial-gradient(#ffffff_1px,transparent_1px)] opacity-20 [background-size:16px_16px]"></div>
              <span className="absolute top-2 right-2 rounded border border-zinc-700 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[8px] text-zinc-300">
                {item.split}
              </span>
              <div className="absolute inset-x-0 bottom-0 flex items-center justify-between border-t border-zinc-800/80 bg-zinc-950/90 px-2.5 py-1.5 font-mono text-[10px] text-zinc-400 backdrop-blur-sm">
                <span className="truncate">{item.filename}</span>
                <span className="shrink-0 transition-colors group-hover:text-emerald-400">
                  {dataset.category === "yolo" ? "editar bbox →" : "ver caption →"}
                </span>
              </div>
            </div>
          ))}
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            disabled={uploading}
            className="flex h-36 flex-col items-center justify-center space-y-1.5 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 transition-all hover:border-emerald-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 disabled:opacity-60"
          >
            <IconPlus className="h-5 w-5" />
            <span className="font-mono text-[10px]">
              {uploading
                ? `Enviando ${uploadCount} arquivo(s)…`
                : "Adicionar imagens"}
            </span>
          </button>
          {items.length < total && (
            <button
              type="button"
              onClick={loadMore}
              disabled={loadingMore}
              className="flex h-36 flex-col items-center justify-center space-y-1.5 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 transition-all hover:border-emerald-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 disabled:opacity-60"
            >
              <span className="font-mono text-[10px]">
                {loadingMore ? "Carregando…" : "Carregar mais"}
              </span>
            </button>
          )}
        </div>
      )}

      <input
        ref={fileRef}
        type="file"
        multiple
        accept="image/*"
        className="hidden"
        onChange={(e) => handleFiles(e.target.files)}
      />
    </div>
  );
}
