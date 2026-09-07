"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { showToast } from "@/components/studio/Toast";
import {
  IconDatabase,
  IconDownload,
  IconFolder,
  IconLayers,
  IconPlay,
  IconPlus,
  IconSearch,
  IconSparkles,
  IconTarget,
  IconTrash,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { getDataset } from "@/lib/datasets";
import { exportDataset, exportErrorMessage } from "@/lib/backup";
import {
  getSearchStatus,
  searchByImage,
  searchDataset,
  triggerSearchIndex,
} from "@/lib/search";
import {
  listImages,
  purgeTrash,
  restoreImage,
  softDeleteImage,
  uploadImages,
} from "@/lib/images";
import { formatBytes } from "@/lib/format";
import type {
  Dataset,
  ImageItem,
  SearchItem,
  SearchStatus,
  StudioClass,
} from "@/types/studio";
import ClassesModal from "@/components/studio/ClassesModal";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import ImportDatasetModal from "@/components/studio/ImportDatasetModal";

const PAGE_LIMIT = 50;

type GalleryView = "ativas" | "trash";

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
  const [classesOpen, setClassesOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [view, setView] = useState<GalleryView>("ativas");
  const [trashTotal, setTrashTotal] = useState(0);
  const [deleting, setDeleting] = useState<ImageItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [purgeOpen, setPurgeOpen] = useState(false);
  const [purgeBusy, setPurgeBusy] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [searchInput, setSearchInput] = useState("");
  const [activeQuery, setActiveQuery] = useState<string | null>(null);
  const [similarFor, setSimilarFor] = useState<string | null>(null);
  const [results, setResults] = useState<SearchItem[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchStatus, setSearchStatus] = useState<SearchStatus | null>(null);
  const [statusFailed, setStatusFailed] = useState(false);
  const [indexBusy, setIndexBusy] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const pollAbortRef = useRef<AbortController | null>(null);

  const load = useCallback(
    async (id: string) => {
      setLoading(true);
      setError(null);
      try {
        const [ds, page, trashPage] = await Promise.all([
          getDataset(id),
          listImages(id, { limit: PAGE_LIMIT, offset: 0 }),
          listImages(id, { limit: 1, offset: 0, deleted: true }),
        ]);
        setDataset(ds);
        setItems(page.items);
        setTotal(page.total);
        setTrashTotal(trashPage.total);
        setView("ativas");
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

  function stopSearchPolling() {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    pollAbortRef.current?.abort();
    pollAbortRef.current = null;
  }

  // Arma o polling do status (2s) — chamado pelo efeito de status E por
  // handleTriggerIndex (review 3f fechamento [MAIOR]: o efeito não re-roda
  // quando só o estado muda; sem isso o badge congelava em "Indexando 0/0").
  function startSearchPolling() {
    if (pollRef.current) return;
    const pollCtrl = new AbortController();
    pollAbortRef.current = pollCtrl;
    pollRef.current = setInterval(async () => {
      try {
        const next = await getSearchStatus(datasetId as string, pollCtrl.signal);
        if (pollCtrl.signal.aborted) return;
        setSearchStatus(next);
        setStatusFailed(false);
        if (next.status !== "indexing") stopSearchPolling();
      } catch {
        if (pollCtrl.signal.aborted) return;
        // Mantém o polling — falha transitória não trava a página.
      }
    }, 2000);
  }

  // Status do índice + polling a cada 2s enquanto indexa.
  useEffect(() => {
    if (!datasetId) return;
    const ctrl = new AbortController();
    let cancelled = false;
    async function fetchStatus() {
      try {
        const st = await getSearchStatus(datasetId as string, ctrl.signal);
        if (cancelled) return;
        setSearchStatus(st);
        setStatusFailed(false);
        if (st.status === "indexing") {
          startSearchPolling();
        } else {
          stopSearchPolling();
        }
      } catch (err) {
        if (cancelled) return;
        if (err instanceof ApiError && err.status === 401) return;
        if (err instanceof DOMException && err.name === "AbortError") return;
        setStatusFailed(true);
      }
    }
    fetchStatus();
    return () => {
      cancelled = true;
      ctrl.abort();
      stopSearchPolling();
    };
    // items.length: re-checa o índice após upload (novas imagens mudam o estado).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [datasetId, items.length]);

  function messageForSearch(err: unknown): string {
    if (err instanceof ApiError && err.message) return err.message;
    return "Falha na busca.";
  }

  async function loadMore() {
    if (!datasetId || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await listImages(datasetId, {
        limit: PAGE_LIMIT,
        offset: items.length,
        deleted: view === "trash",
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
    if (!dataset || view === "trash") return;
    if (dataset.category === "yolo") {
      router.push(`/datasets/${datasetId}/annotate/${item.id}`);
    } else {
      showToast(
        "Revisão de caption chega no AutoLabel (fatia futura).",
        "info",
      );
    }
  }

  async function handleClassesSaved(classes: StudioClass[]) {
    setDataset((prev) => (prev ? { ...prev, classes } : prev));
    if (!datasetId) return;
    try {
      const fresh = await getDataset(datasetId);
      setDataset(fresh);
    } catch {
      // Mantém a atualização otimista — o reload falhou em silêncio.
    }
  }

  function clearSearch() {
    setActiveQuery(null);
    setSimilarFor(null);
    setResults([]);
  }

  async function handleTextSearch(query: string) {
    if (!datasetId) return;
    const q = query.trim();
    if (!q) {
      showToast("Digite um texto para buscar.", "info");
      return;
    }
    setSearching(true);
    try {
      const res = await searchDataset(datasetId, q);
      setResults(res.items);
      setActiveQuery(q);
      setSimilarFor(null);
    } catch (err) {
      if (err instanceof ApiError && err.code === "index_not_ready") {
        setSearchStatus((prev) =>
          prev ? { ...prev, status: "not_indexed" } : prev,
        );
        showToast("Índice vazio — indexe para buscar.", "info");
        return;
      }
      if (err instanceof ApiError && err.code === "embedding_unavailable") {
        showToast("Embedder indisponível — tente novamente.", "error");
        return;
      }
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast(messageForSearch(err), "error");
    } finally {
      setSearching(false);
    }
  }

  async function handleSimilarSearch(item: ImageItem) {
    if (!datasetId) return;
    setSearching(true);
    try {
      const res = await searchByImage(datasetId, item.id);
      setResults(res.items);
      setSimilarFor(item.filename);
      setActiveQuery(null);
    } catch (err) {
      if (err instanceof ApiError && err.code === "index_not_ready") {
        setSearchStatus((prev) =>
          prev ? { ...prev, status: "not_indexed" } : prev,
        );
        showToast("Índice vazio — indexe para buscar.", "info");
        return;
      }
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast(messageForSearch(err), "error");
    } finally {
      setSearching(false);
    }
  }

  async function handleTriggerIndex() {
    if (!datasetId || indexBusy) return;
    setIndexBusy(true);
    try {
      const res = await triggerSearchIndex(datasetId);
      if (res.status === "indexing") {
        setSearchStatus((prev) =>
          prev
            ? { ...prev, status: "indexing" }
            : {
                status: "indexing",
                imagesCount: 0,
                indexedCount: 0,
                model: "ViT-B-32",
                dim: 512,
              },
        );
        // O efeito de status não re-roda só porque o estado mudou (deps:
        // datasetId/items.length) — arma o polling aqui (review 3f [MAIOR]).
        startSearchPolling();
        showToast("Indexação disparada.", "success");
      } else {
        showToast("Nada a indexar — o dataset não tem imagens.", "info");
      }
    } catch (err) {
      showToast(messageForSearch(err), "error");
    } finally {
      setIndexBusy(false);
    }
  }

  async function handleExport() {
    if (!dataset || exporting) return;
    setExporting(true);
    try {
      await exportDataset(dataset.id, dataset.slug);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast(
        err instanceof ApiError
          ? exportErrorMessage(err.code)
          : "Falha ao exportar dataset.",
        "error",
      );
    } finally {
      setExporting(false);
    }
  }

  async function switchView(next: GalleryView) {
    if (!datasetId || next === view) return;
    clearSearch();
    try {
      const page = await listImages(datasetId, {
        limit: PAGE_LIMIT,
        offset: 0,
        deleted: next === "trash",
      });
      setItems(page.items);
      setTotal(page.total);
      if (next === "trash") setTrashTotal(page.total);
      setView(next);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast("Falha ao carregar a lixeira.", "error");
    }
  }

  async function refreshDataset(id: string) {
    try {
      const fresh = await getDataset(id);
      setDataset(fresh);
      setTrashTotal(fresh.trashCount);
    } catch {
      // Mantém o estado otimista — o reload falhou em silêncio.
    }
  }

  // 404 = estado stale (outra aba moveu/restaurado, ou purga concorrente):
  // reconcilia a UI com o servidor em vez de deixar a lista mentindo.
  async function reconcileList(id: string) {
    try {
      const [page, trashPage, fresh] = await Promise.all([
        listImages(id, {
          limit: PAGE_LIMIT,
          offset: 0,
          deleted: view === "trash",
        }),
        listImages(id, { limit: 1, offset: 0, deleted: true }),
        getDataset(id),
      ]);
      setItems(page.items);
      setTotal(page.total);
      setTrashTotal(trashPage.total);
      setDataset(fresh);
      if (view === "trash" && trashPage.total === 0) setView("ativas");
    } catch {
      // Reconciliação best-effort — mantém o estado atual se falhar.
    }
  }

  async function confirmSoftDelete() {
    if (!deleting || !datasetId) return;
    const target = deleting;
    setDeleteBusy(true);
    try {
      await softDeleteImage(datasetId, target.id);
      setItems((prev) => prev.filter((i) => i.id !== target.id));
      setTotal((t) => Math.max(0, t - 1));
      setTrashTotal((t) => t + 1);
      setDeleting(null);
      showToast("Imagem movida para a lixeira.", "info", {
        label: "Desfazer",
        onClick: () => undoRestore(target.id),
      });
      await refreshDataset(datasetId);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      if (err instanceof ApiError && err.status === 404) {
        setDeleting(null);
        await reconcileList(datasetId);
        showToast(
          "A lista foi atualizada — a imagem já não está neste estado.",
          "info",
        );
        return;
      }
      const message =
        err instanceof ApiError && err.message
          ? err.message
          : "Falha ao mover para a lixeira.";
      showToast(message, "error");
    } finally {
      setDeleteBusy(false);
    }
  }

  async function undoRestore(imageId: string) {
    if (!datasetId) return;
    try {
      const res = await restoreImage(datasetId, imageId);
      showToast(
        res.filename ? `Restaurada como ${res.filename}.` : "Imagem restaurada.",
        "success",
      );
      const [page, trashPage] = await Promise.all([
        listImages(datasetId, { limit: PAGE_LIMIT, offset: 0 }),
        listImages(datasetId, { limit: 1, offset: 0, deleted: true }),
      ]);
      setItems(page.items);
      setTotal(page.total);
      setTrashTotal(trashPage.total);
      await refreshDataset(datasetId);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      if (err instanceof ApiError && err.status === 404) {
        await reconcileList(datasetId);
        showToast(
          "A lista foi atualizada — a imagem já não está neste estado.",
          "info",
        );
        return;
      }
      showToast("Falha ao restaurar imagem.", "error");
    }
  }

  async function handleRestore(item: ImageItem) {
    if (!datasetId || restoringId) return;
    setRestoringId(item.id);
    try {
      const res = await restoreImage(datasetId, item.id);
      showToast(
        res.filename ? `Restaurada como ${res.filename}.` : "Imagem restaurada.",
        "success",
      );
      const nextTrash = Math.max(0, trashTotal - 1);
      setTrashTotal(nextTrash);
      if (nextTrash === 0) {
        // Lixeira esvaziou — volta para as ativas (a pill some).
        const [page, fresh] = await Promise.all([
          listImages(datasetId, { limit: PAGE_LIMIT, offset: 0 }),
          getDataset(datasetId),
        ]);
        setItems(page.items);
        setTotal(page.total);
        setDataset(fresh);
        setTrashTotal(fresh.trashCount);
        setView("ativas");
      } else {
        setItems((prev) => prev.filter((i) => i.id !== item.id));
        setTotal((t) => Math.max(0, t - 1));
        await refreshDataset(datasetId);
      }
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      if (err instanceof ApiError && err.status === 404) {
        await reconcileList(datasetId);
        showToast(
          "A lista foi atualizada — a imagem já não está neste estado.",
          "info",
        );
        return;
      }
      showToast("Falha ao restaurar imagem.", "error");
    } finally {
      setRestoringId(null);
    }
  }

  async function confirmPurge() {
    if (!datasetId) return;
    setPurgeBusy(true);
    try {
      await purgeTrash(datasetId);
      showToast("Lixeira esvaziada.", "success");
      setPurgeOpen(false);
      const [page, fresh] = await Promise.all([
        listImages(datasetId, { limit: PAGE_LIMIT, offset: 0 }),
        getDataset(datasetId),
      ]);
      setItems(page.items);
      setTotal(page.total);
      setDataset(fresh);
      setTrashTotal(0);
      setView("ativas");
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      showToast("Falha ao esvaziar lixeira.", "error");
    } finally {
      setPurgeBusy(false);
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
          className="inline-flex h-9 w-fit items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-3 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
        >
          ← Datasets
        </button>
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error ?? "Dataset não encontrado."}</p>
          <button
            type="button"
            onClick={() => datasetId && load(datasetId)}
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
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
            className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg border border-white/10 bg-white/[0.05] p-0 text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
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
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 text-brand-400">
            <IconDatabase className="h-5 w-5" />
          </div>
          <div className="min-w-0 flex-1">
            <h2
              title={dataset.title}
              className="truncate text-base font-bold tracking-tight text-white"
            >
              {dataset.title}
            </h2>
            <p className="font-mono text-xs text-zinc-400">
              {dataset.type} · {dataset.imagesCount.toLocaleString()} imagens ·{" "}
              {formatBytes(dataset.sizeBytes)} · {dataset.source ?? "—"}
            </p>
          </div>
        </div>

        <div className="hidden flex-wrap items-center gap-2 md:flex">
          <button
            type="button"
            onClick={() => setClassesOpen(true)}
            title="Renomear, reordenar, criar ou remover classes"
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconLayers className="h-4 w-4" />
            <span>Classes</span>
          </button>
          <button
            type="button"
            disabled
            title="Preparo assistido chega numa fatia futura."
            className="inline-flex h-9 cursor-not-allowed items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 opacity-55 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconSparkles className="h-4 w-4" />
            <span>AutoLabel</span>
          </button>
          <button
            type="button"
            disabled
            title="Geração automática de boxes chega na fatia 4."
            className="inline-flex h-9 cursor-not-allowed items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 opacity-55 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconTarget className="h-4 w-4" />
            <span>AutoTracker</span>
          </button>
          <button
            type="button"
            onClick={() => setImportOpen(true)}
            title="Importar backup estruturado (.zip)"
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconFolder className="h-4 w-4" />
            <span>Importar</span>
          </button>
          <button
            type="button"
            onClick={handleExport}
            disabled={exporting}
            title="Baixar backup estruturado (.zip)"
            className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconDownload className="h-4 w-4" />
            <span>{exporting ? "Exportando…" : "Exportar"}</span>
          </button>
          <button
            type="button"
            disabled
            title="Treino chega na fatia 4."
            className="inline-flex h-9 cursor-not-allowed items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 opacity-55 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconPlay className="h-4 w-4" />
            <span>Treinar este Dataset</span>
          </button>
        </div>
        <div className="relative md:hidden">
          <button
            type="button"
            onClick={() => setActionsOpen((v) => !v)}
            aria-expanded={actionsOpen}
            aria-label="Ações do dataset"
            title="Ações do dataset"
            className="inline-flex size-9 items-center justify-center rounded-lg border border-white/10 bg-white/[0.05] px-3 text-lg leading-none text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent disabled:pointer-events-none disabled:opacity-55"
          >
            <span aria-hidden="true">⋯</span>
          </button>
          {actionsOpen && (
            <div className="glass-menu absolute right-0 z-30 mt-2 flex w-52 flex-col gap-1 rounded-2xl p-2">
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  setClassesOpen(true);
                }}
                title="Renomear, reordenar, criar ou remover classes"
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300"
              >
                <IconLayers className="h-4 w-4" />
                <span>Classes</span>
              </button>
              <button
                type="button"
                disabled
                title="Preparo assistido chega numa fatia futura."
                className="flex h-9 cursor-not-allowed items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 opacity-60"
              >
                <IconSparkles className="h-4 w-4" />
                <span>AutoLabel</span>
              </button>
              <button
                type="button"
                disabled
                title="Geração automática de boxes chega na fatia 4."
                className="flex h-9 cursor-not-allowed items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 opacity-60"
              >
                <IconTarget className="h-4 w-4" />
                <span>AutoTracker</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  setImportOpen(true);
                }}
                title="Importar backup estruturado (.zip)"
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300"
              >
                <IconFolder className="h-4 w-4" />
                <span>Importar</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  handleExport();
                }}
                disabled={exporting}
                title="Baixar backup estruturado (.zip)"
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 disabled:opacity-55"
              >
                <IconDownload className="h-4 w-4" />
                <span>{exporting ? "Exportando…" : "Exportar"}</span>
              </button>
              <button
                type="button"
                disabled
                title="Treino chega na fatia 4."
                className="flex h-9 cursor-not-allowed items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 opacity-60"
              >
                <IconPlay className="h-4 w-4" />
                <span>Treinar este Dataset</span>
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 rounded-2xl border border-zinc-800/80 bg-zinc-950/60 px-4 py-2.5 font-mono text-[11px] text-zinc-400">
        <span className="font-semibold text-zinc-300">
          {dataset.imagesCount.toLocaleString()} amostras
        </span>
        <span className="h-3 w-px bg-zinc-700"></span>
        <span>
          <span className="text-[#34d399]">
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

      {trashTotal > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => switchView("ativas")}
            aria-pressed={view === "ativas"}
            className={`inline-flex h-9 items-center gap-1.5 rounded-lg border px-4 text-sm font-medium whitespace-nowrap transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 ${
              view === "ativas"
                ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
            }`}
          >
            Ativas{" "}
            <span className="font-mono text-[11px] opacity-70">
              {dataset.imagesCount.toLocaleString()}
            </span>
          </button>
          <button
            type="button"
            onClick={() => switchView("trash")}
            aria-pressed={view === "trash"}
            className={`inline-flex h-9 items-center gap-1.5 rounded-lg border px-4 text-sm font-medium whitespace-nowrap transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 ${
              view === "trash"
                ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
            }`}
          >
            Lixeira{" "}
            <span className="font-mono text-[11px] opacity-70">
              {trashTotal.toLocaleString()}
            </span>
          </button>
          {view === "trash" && (
            <button
              type="button"
              onClick={() => setPurgeOpen(true)}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.12] px-4 text-xs font-medium whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              <IconTrash className="h-3.5 w-3.5" />
              <span>Esvaziar lixeira</span>
            </button>
          )}
        </div>
      )}

      {view === "ativas" && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            handleTextSearch(searchInput);
          }}
          className="flex flex-wrap items-center gap-2"
        >
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-xl border border-zinc-800 bg-zinc-950/60 px-3 py-2 focus-within:border-brand-500/60">
            <IconSearch className="h-4 w-4 shrink-0 text-zinc-500" />
            <label htmlFor="gallery-search" className="sr-only">
              Buscar por texto
            </label>
            <input
              id="gallery-search"
              type="text"
              value={searchInput}
              maxLength={500}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Buscar por texto — ex.: 'defeito de solda'"
              aria-label="Buscar por texto"
              className="min-w-0 flex-1 rounded-lg bg-transparent text-xs text-zinc-200 placeholder:text-zinc-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
            />
          </div>
          <button
            type="submit"
            disabled={searching}
            className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconSearch className="h-4 w-4" />
            <span>{searching ? "Buscando…" : "Buscar"}</span>
          </button>
          <span aria-live="polite">
            {statusFailed || !searchStatus ? (
              <span className="rounded-full border border-zinc-800 bg-zinc-900/60 px-3 py-1.5 text-xs font-medium text-zinc-400">
                Status indisponível
              </span>
            ) : searchStatus.status === "not_indexed" ? (
              <span className="flex flex-wrap items-center gap-2">
                <span className="rounded-full border border-zinc-800 bg-zinc-900/60 px-3 py-1.5 text-xs font-medium text-zinc-400">
                  Sem índice
                </span>
                <button
                  type="button"
                  onClick={handleTriggerIndex}
                  disabled={indexBusy}
                  className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                >
                  {indexBusy ? "Indexando…" : "Indexar agora"}
                </button>
              </span>
            ) : searchStatus.status === "indexing" ? (
              <span className="rounded-full border border-amber-400/40 bg-amber-400/10 px-3 py-1.5 font-mono text-xs font-medium text-amber-300">
                Indexando {searchStatus.indexedCount}/{searchStatus.imagesCount}
              </span>
            ) : searchStatus.status === "ready" ? (
              <span className="rounded-full border border-[#34d399]/30 bg-[#34d399]/10 px-3 py-1.5 text-xs font-medium text-[#a7f3d0]">
                Busca pronta
              </span>
            ) : (
              <span className="rounded-full border border-zinc-800 bg-zinc-900/60 px-3 py-1.5 text-xs font-medium text-zinc-400">
                Índice desatualizado?
              </span>
            )}
          </span>
        </form>
      )}

      {view === "trash" ? (
        total === 0 ? (
          <div className="glass-card rounded-2xl border border-zinc-800 px-4 py-14 text-center">
            <p className="text-sm text-zinc-300">A lixeira está vazia.</p>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
            {items.map((item) => (
              <div
                key={item.id}
                className="group relative h-24 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/90 transition-all hover:border-brand-500/60"
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
                <button
                  type="button"
                  onClick={() => handleRestore(item)}
                  disabled={restoringId === item.id}
                  aria-label={`Restaurar ${item.filename}`}
                  className="absolute top-2 left-2 rounded-lg border border-[#34d399]/40 bg-zinc-950/90 px-2 py-1 font-mono text-[10px] font-medium text-[#a7f3d0] transition-colors hover:bg-[#34d399]/20 disabled:opacity-60"
                >
                  {restoringId === item.id ? "Restaurando…" : "Restaurar"}
                </button>
                <div className="absolute inset-x-0 bottom-0 flex items-center justify-between border-t border-zinc-800/80 bg-zinc-950/90 px-2.5 py-1.5 font-mono text-[10px] text-zinc-400 backdrop-blur-sm">
                  <span title={item.filename} className="truncate">{item.filename}</span>
                </div>
              </div>
            ))}
            {items.length < total && (
              <button
                type="button"
                onClick={loadMore}
                disabled={loadingMore}
                className="flex h-24 flex-col items-center justify-center space-y-1.5 md:h-36 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 transition-all hover:border-brand-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 disabled:opacity-60"
              >
                <span className="font-mono text-[10px]">
                  {loadingMore ? "Carregando…" : "Carregar mais"}
                </span>
              </button>
            )}
          </div>
        )
      ) : dataset.imagesCount === 0 ? (
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
            className="mt-4 inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            {uploading ? `Enviando ${uploadCount} arquivo(s)…` : "Enviar amostras"}
          </button>
        </div>
      ) : activeQuery !== null || similarFor !== null ? (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-zinc-800/80 bg-zinc-950/60 px-4 py-2.5 text-[11px] text-zinc-400">
            <span>
              Resultados da busca —{" "}
              <span className="font-mono text-zinc-200">
                {results.length.toLocaleString()}
              </span>{" "}
              {results.length === 1 ? "imagem" : "imagens"}
            </span>
            <button
              type="button"
              onClick={clearSearch}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              Limpar busca
            </button>
          </div>
          {results.length === 0 ? (
            <div className="glass-card rounded-2xl border border-zinc-800 px-4 py-14 text-center">
              <p className="text-sm text-zinc-300">
                {activeQuery !== null
                  ? `Nenhum resultado para '${activeQuery}'`
                  : "Nenhuma imagem similar"}
              </p>
            </div>
          ) : (
            <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
              {results.map((result) => (
                <div
                  key={result.image.id}
                  onClick={() =>
                    router.push(
                      `/datasets/${datasetId}/annotate/${result.image.id}`,
                    )
                  }
                  className="group relative h-24 cursor-pointer overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/90 transition-all hover:border-brand-500/60"
                >
                  <img
                    src={result.image.url}
                    alt={result.image.filename}
                    loading="lazy"
                    className="absolute inset-0 h-full w-full object-cover"
                  />
                  <div className="absolute inset-0 bg-[radial-gradient(#ffffff_1px,transparent_1px)] opacity-20 [background-size:16px_16px]"></div>
                  <span
                    title="Similaridade (cosseno, -1..1)"
                    className="absolute top-2 right-2 rounded border border-brand-500/30 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[10px] text-brand-300"
                  >
                    {result.score.toFixed(2)}
                  </span>
                  <div className="absolute inset-x-0 bottom-0 flex items-center justify-between border-t border-zinc-800/80 bg-zinc-950/90 px-2.5 py-1.5 font-mono text-[10px] text-zinc-400 backdrop-blur-sm">
                    <span title={result.image.filename} className="truncate">{result.image.filename}</span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
          {items.map((item) => (
            <div
              key={item.id}
              onClick={() => handleTileClick(item)}
              className="group relative h-24 cursor-pointer overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/90 transition-all hover:border-brand-500/60"
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
              <div className="absolute top-2 left-2 flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    setDeleting(item);
                  }}
                  aria-label={`Mover ${item.filename} para a lixeira`}
                  title="Mover para a lixeira"
                  className="rounded-lg border border-rose-500/40 bg-zinc-950/90 p-1.5 text-rose-300 opacity-0 transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-rose-500/20"
                >
                  <IconTrash className="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleSimilarSearch(item);
                  }}
                  aria-label={`Buscar similares de ${item.filename}`}
                  title="Buscar similares"
                  className="rounded-lg border border-brand-500/40 bg-zinc-950/90 p-1.5 text-brand-300 opacity-0 transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-brand-500/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
                >
                  <IconSearch className="h-3.5 w-3.5" />
                </button>
              </div>
              <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 border-t border-zinc-800/80 bg-zinc-950/90 px-2.5 py-1.5 font-mono text-[10px] text-zinc-400 backdrop-blur-sm">
                <span title={item.filename} className="min-w-0 flex-1 truncate">{item.filename}</span>
                <span title={dataset.category === "yolo" ? "Editar bounding boxes" : "Ver caption"} className="shrink-0 truncate transition-colors group-hover:text-brand-400">
                  {dataset.category === "yolo" ? "editar bbox →" : "ver caption →"}
                </span>
              </div>
            </div>
          ))}
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            disabled={uploading}
            className="flex h-24 flex-col items-center justify-center space-y-1.5 md:h-36 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 transition-all hover:border-brand-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 disabled:opacity-60"
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
              className="flex h-24 flex-col items-center justify-center space-y-1.5 md:h-36 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 transition-all hover:border-brand-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 disabled:opacity-60"
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

      {classesOpen && (
        <ClassesModal
          datasetId={datasetId as string}
          datasetClasses={dataset.classes}
          onClose={() => setClassesOpen(false)}
          onSaved={handleClassesSaved}
        />
      )}
      {importOpen && <ImportDatasetModal onClose={() => setImportOpen(false)} />}
      <ConfirmDialog
        open={deleting !== null}
        title="Mover para a lixeira"
        body={
          deleting ? (
            <p>
              <strong className="text-zinc-100">{deleting.filename}</strong>{" "}
              vai para a lixeira — restaurável até você esvaziá-la.
            </p>
          ) : null
        }
        confirmLabel="Mover para a lixeira"
        danger
        busy={deleteBusy}
        onConfirm={confirmSoftDelete}
        onClose={() => {
          if (!deleteBusy) setDeleting(null);
        }}
      />
      <ConfirmDialog
        open={purgeOpen}
        title="Esvaziar lixeira"
        body={
          <p>
            Exclusão permanente — não há como restaurar depois.{" "}
            <span className="font-mono">
              {trashTotal.toLocaleString()}{" "}
              {trashTotal === 1 ? "imagem será removida" : "imagens serão removidas"}
            </span>{" "}
            para sempre.
          </p>
        }
        confirmLabel="Esvaziar lixeira"
        danger
        busy={purgeBusy}
        onConfirm={confirmPurge}
        onClose={() => {
          if (!purgeBusy) setPurgeOpen(false);
        }}
      />
    </div>
  );
}
