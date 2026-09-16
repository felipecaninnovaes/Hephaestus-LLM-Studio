"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import {
  Button,
  ConfirmDialog,
  DropOverlay,
  EmptyState,
  SearchInput,
  SubmodulePills,
  Spinner,
  showToast,
  useFileDrop,
} from "@/components/ui";
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
import {
  autoTrackDisabledReason,
  canAutoTrack,
  canTrainYolo,
  getDataset,
  trainDisabledReason,
} from "@/lib/datasets";
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
  type UploadResultItem,
} from "@/lib/images";
import { formatBytes } from "@/lib/format";
import { extractFilesFromDataTransfer } from "@/lib/dataset-inspector";
import type {
  Dataset,
  ImageItem,
  SearchItem,
  SearchStatus,
  StudioClass,
} from "@/types/studio";
import ClassesModal from "@/components/studio/ClassesModal";
import ImportDatasetModal from "@/components/studio/ImportDatasetModal";
import TrainYoloModal from "@/components/studio/TrainYoloModal";
import AutoTrackerModal from "@/components/studio/AutoTrackerModal";
import AutoLabelModal from "@/components/studio/AutoLabelModal";
import ImageCard from "@/components/studio/ImageCard";
import UploadFloatingDock from "@/components/studio/UploadFloatingDock";
import UploadAuditModal from "@/components/studio/UploadAuditModal";
import GalleryOperateToolbar, {
  type GallerySplitView,
  type GalleryAnnotationFilter,
  type GalleryDensity,
} from "@/components/studio/GalleryOperateToolbar";
import FloatingSelectionBar from "@/components/studio/FloatingSelectionBar";
import BatchEditClassesModal from "@/components/studio/BatchEditClassesModal";
import ImageTableView from "@/components/studio/ImageTableView";
import ImageQuickLookModal from "@/components/studio/ImageQuickLookModal";

const PAGE_LIMIT = 50;

type GalleryView = "ativas" | "trash";

export default function DatasetGalleryPage() {
  const params = useParams<{ id?: string }>();
  const datasetId = params?.id;
  const router = useRouter();
  const fileRef = useRef<HTMLInputElement>(null);

  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [items, setItems] = useState<ImageItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadCount, setUploadCount] = useState(0);
  const [uploadSent, setUploadSent] = useState(0);
  const [uploadBatchInfo, setUploadBatchInfo] = useState<{ batchIndex: number; batchCount: number } | null>(null);
  const uploadCancelledRef = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const [classesOpen, setClassesOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [view, setView] = useState<GalleryView>("ativas");
  const [splitView, setSplitView] = useState<GallerySplitView>("all");
  const [annotationFilter, setAnnotationFilter] = useState<GalleryAnnotationFilter>("all");
  const [density, setDensity] = useState<GalleryDensity>("normal");
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [quickLookIndex, setQuickLookIndex] = useState<number | null>(null);
  const [auditModalOpen, setAuditModalOpen] = useState(false);
  const [lastUploadResults, setLastUploadResults] = useState<UploadResultItem[] | null>(null);
  const [batchDeleteBusy, setBatchDeleteBusy] = useState(false);
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const [trashTotal, setTrashTotal] = useState(0);
  const [deleting, setDeleting] = useState<ImageItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [purgeOpen, setPurgeOpen] = useState(false);
  const [purgeBusy, setPurgeBusy] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [trainOpen, setTrainOpen] = useState(false);
  const [autoTrackerOpen, setAutoTrackerOpen] = useState(false);
  const [autoLabelOpen, setAutoLabelOpen] = useState(false);
  const [batchEditClassesOpen, setBatchEditClassesOpen] = useState(false);
  const [selectedClassId, setSelectedClassId] = useState<string | null>(null);
  const [searchMode, setSearchMode] = useState<"tag" | "semantic">("tag");
  const [searchInput, setSearchInput] = useState("");
  const [activeTag, setActiveTag] = useState<string | null>(null);
  const [activeQuery, setActiveQuery] = useState<string | null>(null);
  const [similarFor, setSimilarFor] = useState<string | null>(null);
  const [results, setResults] = useState<SearchItem[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchStatus, setSearchStatus] = useState<SearchStatus | null>(null);
  const [statusFailed, setStatusFailed] = useState(false);
  const [indexBusy, setIndexBusy] = useState(false);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const pollAbortRef = useRef<AbortController | null>(null);
  const searchTimerRef = useRef<number | null>(null);
  const { isDragging: isDraggingPage, dropProps } = useFileDrop({
    onDropFiles: async (files, dataTransfer) => {
      try {
        const extracted = await extractFilesFromDataTransfer(dataTransfer);
        if (extracted.length > 0) {
          await handleFiles(extracted);
          return;
        }
      } catch {
        // fallback para arquivos diretos
      }
      await handleFiles(files);
    },
  });

  const load = useCallback(
    async (
      id: string,
      currentSplit: GallerySplitView = splitView,
      currentAnnotation: GalleryAnnotationFilter = annotationFilter,
      currentClassId: string | null = selectedClassId,
      currentTag: string | null = activeTag,
    ) => {
      setLoading(true);
      setError(null);
      try {
        const isTrash = currentSplit === "trash";
        const splitParam =
          currentSplit === "all" || currentSplit === "trash"
            ? undefined
            : currentSplit;
        const labeledParam =
          currentAnnotation === "all"
            ? undefined
            : currentAnnotation === "labeled";

        const [ds, page, trashPage] = await Promise.all([
          getDataset(id),
          listImages(id, {
            limit: PAGE_LIMIT,
            offset: 0,
            split: splitParam,
            labeled: labeledParam,
            deleted: isTrash,
            classId: currentClassId || undefined,
            tag: currentTag || undefined,
          }),
          listImages(id, { limit: 1, offset: 0, deleted: true }),
        ]);
        setDataset(ds);
        setItems(page.items);
        setTotal(page.total);
        setTrashTotal(trashPage.total);
        setView(isTrash ? "trash" : "ativas");
        setSelectedIds(new Set());
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
    [router, splitView, annotationFilter, selectedClassId, activeTag],
  );

  useEffect(() => {
    if (datasetId && !(searchMode === "semantic" && (activeQuery || similarFor))) {
      void load(datasetId, splitView, annotationFilter, selectedClassId, activeTag);
    }
  }, [datasetId, load, splitView, annotationFilter, selectedClassId, activeTag, searchMode, activeQuery, similarFor]);

  // Atualiza galeria e classes quando jobs em segundo plano (AutoTracker, etc) aplicam alterações
  useEffect(() => {
    function handleDatasetUpdated(e: Event) {
      const ce = e as CustomEvent<{ datasetId?: string }>;
      if (datasetId && (!ce.detail || ce.detail.datasetId === datasetId)) {
        void load(datasetId, splitView, annotationFilter, selectedClassId, activeTag);
      }
    }
    window.addEventListener("hephaestus:dataset-updated", handleDatasetUpdated);
    return () => {
      window.removeEventListener("hephaestus:dataset-updated", handleDatasetUpdated);
    };
  }, [datasetId, load, splitView, annotationFilter, selectedClassId, activeTag]);

  function stopSearchPolling() {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
    pollAbortRef.current?.abort();
    pollAbortRef.current = null;
  }

  // Arma o polling do status (2s) — ciente de visibilidade da aba
  function startSearchPolling() {
    if (pollRef.current) return;
    const pollCtrl = new AbortController();
    pollAbortRef.current = pollCtrl;
    pollRef.current = setInterval(async () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
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

  // Busca dinâmica com debounce (400ms) — dispara quando searchInput ou searchMode muda.
  useEffect(() => {
    if (searchTimerRef.current) {
      window.clearTimeout(searchTimerRef.current);
      searchTimerRef.current = null;
    }
    const q = searchInput.trim();
    if (!q) {
      if (searchMode === "semantic" && activeQuery) {
        clearSearch();
      } else if (searchMode === "tag" && activeTag) {
        setActiveTag(null);
      }
      return;
    }
    searchTimerRef.current = window.setTimeout(() => {
      void handleTextSearch(searchInput, searchMode);
    }, 400);
    return () => {
      if (searchTimerRef.current) {
        window.clearTimeout(searchTimerRef.current);
        searchTimerRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchInput, searchMode]);

  function messageForSearch(err: unknown): string {
    if (err instanceof ApiError && err.message) return err.message;
    return "Falha na busca.";
  }

  async function loadMore() {
    if (!datasetId || loadingMore) return;
    setLoadingMore(true);
    try {
      const isTrash = splitView === "trash";
      const splitParam =
        splitView === "all" || splitView === "trash" ? undefined : splitView;
      const labeledParam =
        annotationFilter === "all"
          ? undefined
          : annotationFilter === "labeled";

      const page = await listImages(datasetId, {
        limit: PAGE_LIMIT,
        offset: items.length,
        split: splitParam,
        labeled: labeledParam,
        deleted: isTrash,
        classId: selectedClassId || undefined,
        tag: searchMode === "tag" && activeTag ? activeTag : undefined,
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

  // Infinite Scroll via IntersectionObserver no sentinela ao final da lista
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (
          entries[0]?.isIntersecting &&
          !loading &&
          !loadingMore &&
          items.length < total
        ) {
          void loadMore();
        }
      },
      { rootMargin: "350px" },
    );

    observer.observe(el);
    return () => observer.disconnect();
  }, [loading, loadingMore, items.length, total]);

  async function handleFiles(files: FileList | File[] | null) {
    if (!files || (Array.isArray(files) ? files.length === 0 : files.length === 0) || !datasetId || uploading) return;
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
          setUploadBatchInfo({ batchIndex: p.batchIndex, batchCount: p.batchCount });
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
      await load(datasetId, splitView, annotationFilter);

      const wasCancelled = uploadCancelledRef.current;
      if (wasCancelled) {
        const summary = problem.length > 0
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
        const suffix = problem.length > 3 ? ` (+${problem.length - 3} mais)` : "";
        showToast(
          `${stored.length} enviadas, ${problem.length} rejeitadas: ${examples}${suffix}`,
          problem.length > stored.length ? "error" : "info",
        );
        // Abre auditoria se houver rejeições para inspeção detalhada
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
  }

  const handleSplitChange = (nextSplit: GallerySplitView) => {
    setSplitView(nextSplit);
    setView(nextSplit === "trash" ? "trash" : "ativas");
  };

  const handleAnnotationChange = (nextFilter: GalleryAnnotationFilter) => {
    setAnnotationFilter(nextFilter);
  };

  const handleSelectToggle = (id: string, selected: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (selected) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  const isAllSelected = items.length > 0 && items.every((i) => selectedIds.has(i.id));

  const handleSelectAll = () => {
    setSelectedIds(new Set(items.map((i) => i.id)));
    setSelectionMode(true);
  };

  const handleClearSelection = () => {
    setSelectedIds(new Set());
    setSelectionMode(false);
  };

  const handleConfirmBatchDelete = async () => {
    if (!datasetId || selectedIds.size === 0) return;
    setBatchDeleteBusy(true);
    try {
      const ids = Array.from(selectedIds);
      let count = 0;
      for (const id of ids) {
        try {
          await softDeleteImage(datasetId, id);
          count++;
        } catch {
          // segue para a próxima imagem
        }
      }
      showToast(
        `${count} ${count === 1 ? "imagem movida" : "imagens movidas"} para a lixeira.`,
        "success",
      );
      setSelectedIds(new Set());
      setSelectionMode(false);
      setBatchDeleteOpen(false);
      await load(datasetId, splitView, annotationFilter);
    } catch {
      showToast("Erro ao mover imagens para a lixeira.", "error");
    } finally {
      setBatchDeleteBusy(false);
    }
  };

  const handleOpenQuickLook = (item: ImageItem) => {
    const idx = items.findIndex((i) => i.id === item.id);
    if (idx >= 0) {
      setQuickLookIndex(idx);
    }
  };

  function handleTileClick(item: ImageItem) {
    if (!dataset || view === "trash") return;
    if (dataset.category === "yolo") {
      router.push(`/datasets/${datasetId}/annotate/${item.id}`);
    } else {
      handleOpenQuickLook(item);
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
    setActiveTag(null);
    setSimilarFor(null);
    setResults([]);
    setSearchInput("");
  }

  function clearAllFilters() {
    clearSearch();
    setSelectedClassId(null);
  }

  async function handleTextSearch(
    query: string,
    mode: "tag" | "semantic" = searchMode,
  ) {
    if (!datasetId) return;
    const q = query.trim();
    if (!q) {
      clearSearch();
      return;
    }

    if (mode === "tag") {
      setResults([]);
      setSimilarFor(null);
      setActiveQuery(null);
      setActiveTag(q);
      return;
    }

    setSearching(true);
    try {
      const res = await searchDataset(datasetId, q);
      setResults(res.items);
      setActiveQuery(q);
      setActiveTag(null);
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
        <p className="py-10 text-center font-mono text-xs text-zinc-400">Carregando galeria…</p>
      </div>
    );
  }

  if (error || !dataset) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <Button
          type="button"
          variant="ghost"
          size="md"
          onClick={() => router.push("/datasets")}
          className="w-fit"
        >
          ← Datasets
        </Button>
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error ?? "Dataset não encontrado."}</p>
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => datasetId && load(datasetId)}
          >
            Tentar novamente
          </Button>
        </div>
      </div>
    );
  }

  const reviewer = dataset.category === "yolo" ? "AutoTracker" : "AutoLabel";

  return (
    <div
      {...dropProps}
      className="relative mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6"
    >
      {/* Overlay Óptico de Drag & Drop para Upload de Imagens */}
      <DropOverlay
        open={isDraggingPage}
        title="Solte as imagens aqui"
        subtitle={`Upload direto de amostras para o dataset ${dataset.title}`}
        onDragLeave={dropProps.onDragLeave}
        onDrop={dropProps.onDrop}
      />


      <div className="flex flex-col justify-between gap-4 md:flex-row md:items-center">
        <div className="flex items-center space-x-3">
          <Button
            type="button"
            variant="secondary"
            size="icon"
            onClick={() => router.push("/datasets")}
            title="Voltar para a lista de datasets"
            aria-label="Voltar para a lista de datasets"
          >
            <svg
              className="size-4"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.7"
              viewBox="0 0 24 24"
            >
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </Button>
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm text-brand-400">
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
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => setClassesOpen(true)}
            title="Renomear, reordenar, criar ou remover classes"
          >
            <IconLayers className="h-4 w-4" />
            <span>Classes</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={dataset.imagesCount === 0}
            title={
              dataset.imagesCount === 0
                ? "Dataset não contém imagens."
                : "Gerar legendas em lote com AutoLabel"
            }
            onClick={() => dataset.imagesCount > 0 && setAutoLabelOpen(true)}
          >
            <IconSparkles className="h-4 w-4" />
            <span>AutoLabel</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canAutoTrack(dataset)}
            title={autoTrackDisabledReason(dataset)}
            onClick={() => canAutoTrack(dataset) && setAutoTrackerOpen(true)}
          >
            <IconTarget className="h-4 w-4" />
            <span>AutoTracker</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => setImportOpen(true)}
            title="Importar backup estruturado (.zip)"
          >
            <IconFolder className="h-4 w-4" />
            <span>Importar</span>
          </Button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={handleExport}
            disabled={exporting}
            loading={exporting}
            title="Baixar backup estruturado (.zip)"
          >
            <IconDownload className="h-4 w-4" />
            <span>{exporting ? "Exportando…" : "Exportar"}</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canTrainYolo(dataset)}
            title={trainDisabledReason(dataset)}
            onClick={() => canTrainYolo(dataset) && setTrainOpen(true)}
          >
            <IconPlay className="h-4 w-4" />
            <span>Treinar este Dataset</span>
          </Button>
        </div>
        <div className="relative md:hidden">
          <Button
            type="button"
            variant="secondary"
            size="icon"
            onClick={() => setActionsOpen((v) => !v)}
            aria-expanded={actionsOpen}
            aria-label="Ações do dataset"
            title="Ações do dataset"
          >
            <span aria-hidden="true" className="text-lg leading-none">⋯</span>
          </Button>
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
                disabled={dataset.imagesCount === 0}
                title={
                  dataset.imagesCount === 0
                    ? "Dataset não contém imagens."
                    : "Gerar legendas em lote com AutoLabel"
                }
                onClick={() => {
                  if (dataset.imagesCount === 0) return;
                  setActionsOpen(false);
                  setAutoLabelOpen(true);
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  dataset.imagesCount > 0
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
              >
                <IconSparkles className="h-4 w-4" />
                <span>AutoLabel</span>
              </button>
              <button
                type="button"
                disabled={!canAutoTrack(dataset)}
                title={autoTrackDisabledReason(dataset)}
                onClick={() => {
                  if (!canAutoTrack(dataset)) return;
                  setActionsOpen(false);
                  setAutoTrackerOpen(true);
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  canAutoTrack(dataset)
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
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
                disabled={!canTrainYolo(dataset)}
                title={trainDisabledReason(dataset)}
                onClick={() => {
                  if (!canTrainYolo(dataset)) return;
                  setActionsOpen(false);
                  setTrainOpen(true);
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  canTrainYolo(dataset)
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
              >
                <IconPlay className="h-4 w-4" />
                <span>Treinar este Dataset</span>
              </button>
            </div>
          )}
        </div>
      </div>

      <div className="glass-card flex flex-wrap items-center gap-2 rounded-2xl shadow-lg px-4 py-2.5 font-mono text-2xs text-zinc-400">
        <span className="font-semibold text-zinc-200">
          {dataset.imagesCount.toLocaleString()} amostras
        </span>
        <span className="h-3 w-px bg-white/10"></span>
        <span>
          <span className="text-status-success font-semibold">
            {dataset.labeledCount.toLocaleString()}
          </span>{" "}
          rotuladas por {reviewer}
        </span>
        <span className="h-3 w-px bg-white/10"></span>
        <span>Formato: {dataset.format}</span>
        <span className="h-3 w-px bg-white/10"></span>
        <span>
          Backup: exportar/importar mantém JSON/YAML de config + anotações
        </span>
      </div>

      <GalleryOperateToolbar
        currentView={splitView}
        onViewChange={handleSplitChange}
        totalActive={dataset.imagesCount}
        totalTrash={trashTotal}
        annotationFilter={annotationFilter}
        onAnnotationFilterChange={handleAnnotationChange}
        density={density}
        onDensityChange={setDensity}
        selectionMode={selectionMode}
        onToggleSelectionMode={() => {
          if (selectionMode) {
            setSelectedIds(new Set());
            setSelectionMode(false);
          } else {
            setSelectionMode(true);
          }
        }}
        selectedCount={selectedIds.size}
        isAllSelected={isAllSelected}
        onSelectAll={handleSelectAll}
        onClearSelection={handleClearSelection}
        classes={dataset.classes ?? []}
        selectedClassId={selectedClassId}
        onClassChange={(clsId) => {
          setSelectedClassId(clsId);
        }}
      />

      {splitView !== "trash" && (
        <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-2">
          {/* Alternador de Modo de Filtro / Busca */}
          <div className="inline-flex rounded-xl border border-white/10 bg-zinc-950/80 p-0.5 shrink-0">
            <button
              type="button"
              onClick={() => {
                setSearchMode("tag");
                setActiveQuery(null);
                setResults([]);
                if (searchInput.trim()) {
                  setActiveTag(searchInput.trim());
                }
              }}
              title="Filtro Estrito: busca unicamente imagens que possuem esta tag, classe ou legenda"
              className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs transition-colors cursor-pointer ${
                searchMode === "tag"
                  ? "bg-brand-500/20 text-brand-300 font-semibold shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <span>🏷️ Filtro Estrito (Tag/Classe)</span>
            </button>
            <button
              type="button"
              onClick={() => {
                setSearchMode("semantic");
                setActiveTag(null);
                if (searchInput.trim()) {
                  void handleTextSearch(searchInput, "semantic");
                }
              }}
              title="Busca Semântica por IA: aproximação vetorial de conceitos visuais (CLIP)"
              className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs transition-colors cursor-pointer ${
                searchMode === "semantic"
                  ? "bg-sky-500/20 text-sky-300 font-semibold shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <span>✨ Similaridade IA (CLIP)</span>
            </button>
          </div>

          <div className="min-w-0 flex-1">
            <SearchInput
              id="gallery-search"
              size="lg"
              loading={searching}
              value={searchInput}
              maxLength={500}
              onChange={(e) => setSearchInput(e.target.value)}
              onClear={clearSearch}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  if (searchTimerRef.current)
                    window.clearTimeout(searchTimerRef.current);
                  void handleTextSearch(searchInput, searchMode);
                }
              }}
              placeholder={
                searchMode === "tag"
                  ? "Filtrar por tag ou classe exata — ex.: 'capacete', 'defeito', 'carro'…"
                  : "Buscar por similaridade semântica AI — ex.: 'foto noturna com luz suave'…"
              }
              aria-label="Buscar imagens"
            />
          </div>
          {searchMode === "semantic" && searchStatus?.status === "indexing" && (
            <span
              aria-live="polite"
              title="Indexação de busca semântica em andamento"
              className="cursor-default rounded-full border border-amber-400/40 bg-amber-400/10 backdrop-blur-sm px-3 py-1.5 font-mono text-xs font-medium text-amber-300 shrink-0"
            >
              Indexando {searchStatus.indexedCount}/{searchStatus.imagesCount}
            </span>
          )}
          {searchMode === "semantic" && searchStatus?.status === "not_indexed" && (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={handleTriggerIndex}
              disabled={indexBusy}
              loading={indexBusy}
              title="Gerar embeddings de busca semântica para este dataset"
              className="shrink-0"
            >
              {indexBusy ? "Indexando…" : "Indexar busca"}
            </Button>
          )}
        </div>
      )}

      {/* Barra de Filtro Estrito Ativo */}
      {searchMode === "tag" && (activeTag || selectedClassId) && (
        <div className="flex items-center justify-between gap-2 rounded-xl border border-brand-500/25 bg-brand-500/5 px-3.5 py-2 font-mono text-xs text-zinc-300">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-semibold text-brand-300">Filtro Ativo:</span>
            {activeTag && (
              <span className="rounded bg-brand-500/20 px-2 py-0.5 text-brand-200">
                Tag &quot;{activeTag}&quot;
              </span>
            )}
            {selectedClassId && (
              <span className="rounded bg-status-alert/20 px-2 py-0.5 text-amber-200">
                Classe: {dataset.classes?.find((c) => c.id === selectedClassId)?.name ?? selectedClassId}
              </span>
            )}
            <span className="text-zinc-400">({total.toLocaleString()} imagens encontradas)</span>
          </div>
          <button
            type="button"
            onClick={clearAllFilters}
            className="text-2xs text-zinc-400 hover:text-white underline cursor-pointer shrink-0"
          >
            Limpar filtros
          </button>
        </div>
      )}

      {splitView === "trash" ? (
        total === 0 ? (
          <EmptyState
            icon={<IconTrash className="h-5 w-5" />}
            title="A lixeira está vazia"
            description="Imagens movidas para a lixeira aparecerão aqui antes da exclusão definitiva."
            className="py-14 border border-zinc-800"
          />
        ) : density === "table" ? (
          <ImageTableView
            items={items}
            selectedIds={selectedIds}
            onToggleSelect={handleSelectToggle}
            onSelectAll={handleSelectAll}
            onClearSelection={handleClearSelection}
            onQuickLook={(idx) => setQuickLookIndex(idx)}
            category={dataset.category}
          />
        ) : (
          <div
            className={
              density === "compact"
                ? "grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-6 xl:grid-cols-8"
                : "grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4"
            }
          >
            {items.map((item) => (
              <ImageCard
                key={item.id}
                item={item}
                variant="trash"
                density={density}
                selected={selectedIds.has(item.id)}
                selectionMode={selectionMode}
                onSelect={(sel) => handleSelectToggle(item.id, sel)}
                onRestore={() => handleRestore(item)}
                isRestoring={restoringId === item.id}
                onQuickLook={() => handleOpenQuickLook(item)}
              />
            ))}
          </div>
        )
      ) : dataset.imagesCount === 0 ? (
        <EmptyState
          icon={<IconFolder className="h-5 w-5" />}
          title="Galeria vazia"
          description="Este dataset ainda não tem amostras. Arraste uma pasta ou envie imagens pelo botão abaixo."
          className="py-14 border border-zinc-800"
        >
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => fileRef.current?.click()}
            disabled={uploading}
            loading={uploading && !uploadSent}
            className="mt-2"
          >
            {uploading
              ? `Enviando ${uploadSent} de ${uploadCount}…`
              : "Enviar amostras"}
          </Button>
          {uploading && uploadBatchInfo && (
            <p className="mt-1.5 font-mono text-2xs text-zinc-400">
              lote {uploadBatchInfo.batchIndex} de {uploadBatchInfo.batchCount}
            </p>
          )}
          {uploading && (
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => { uploadCancelledRef.current = true; }}
              className="mt-2"
            >
              Cancelar envio
            </Button>
          )}
        </EmptyState>
      ) : similarFor !== null || (searchMode === "semantic" && activeQuery !== null) ? (
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-zinc-800/80 bg-zinc-950/60 backdrop-blur-sm px-4 py-2.5 text-2xs text-zinc-400">
            <span>
              Resultados da busca semântica —{" "}
              <span className="font-mono text-zinc-200">
                {results.length.toLocaleString()}
              </span>{" "}
              {results.length === 1 ? "imagem" : "imagens"}
            </span>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={clearSearch}
            >
              Limpar busca
            </Button>
          </div>
          {results.length === 0 ? (
            <EmptyState
              icon={<IconSearch className="h-5 w-5" />}
              title={
                activeQuery !== null
                  ? `Nenhum resultado para '${activeQuery}'`
                  : "Nenhuma imagem similar"
              }
              description="Tente ajustar os termos de busca ou utilize uma imagem diferente como referência."
              className="py-14 border border-zinc-800"
            />
          ) : (
            <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
              {results.map((result) => (
                <ImageCard
                  key={result.image.id}
                  item={result.image}
                  variant="search"
                  searchScore={result.score}
                  density={density === "compact" ? "compact" : "normal"}
                  onClick={() =>
                    router.push(
                      `/datasets/${datasetId}/annotate/${result.image.id}`,
                    )
                  }
                  onQuickLook={() => handleOpenQuickLook(result.image)}
                />
              ))}
            </div>
          )}
        </div>
      ) : total === 0 ? (
        <EmptyState
          icon={activeTag || selectedClassId ? <IconSearch className="h-5 w-5" /> : <IconFolder className="h-5 w-5" />}
          title={
            activeTag || selectedClassId
              ? "Nenhuma imagem encontrada com o filtro aplicado"
              : annotationFilter !== "all"
              ? "Nenhuma imagem encontrada com o filtro de rótulo"
              : "Nenhuma imagem ativa neste conjunto"
          }
          description={
            activeTag || selectedClassId
              ? "Tente buscar por outro termo ou selecione outra classe."
              : "Ajuste os filtros da barra de ferramentas ou adicione mais amostras."
          }
          className="py-14 border border-zinc-800"
        >
          {(activeTag || selectedClassId) && (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={clearAllFilters}
              className="mt-2"
            >
              Limpar filtros
            </Button>
          )}
        </EmptyState>
      ) : density === "table" ? (
        <div className="flex flex-col gap-4">
          <ImageTableView
            items={items}
            selectedIds={selectedIds}
            onToggleSelect={handleSelectToggle}
            onSelectAll={handleSelectAll}
            onClearSelection={handleClearSelection}
            onOpenAnnotate={handleTileClick}
            onDelete={(img) => setDeleting(img)}
            onQuickLook={(idx) => setQuickLookIndex(idx)}
            category={dataset.category}
          />
        </div>
      ) : (
        <div
          className={
            density === "compact"
              ? "grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-6 xl:grid-cols-8"
              : "grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4"
          }
        >
          {items.map((item) => (
            <ImageCard
              key={item.id}
              item={item}
              variant="active"
              density={density}
              selected={selectedIds.has(item.id)}
              selectionMode={selectionMode}
              onSelect={(sel) => handleSelectToggle(item.id, sel)}
              onClick={() => handleTileClick(item)}
              onDelete={() => setDeleting(item)}
              onSearchSimilar={() => handleSimilarSearch(item)}
              onQuickLook={() => handleOpenQuickLook(item)}
              actionText={
                dataset.category === "yolo" ? "editar bbox →" : "ver legenda →"
              }
            />
          ))}
          <div className="relative inline-flex">
            <div
              role="button"
              tabIndex={uploading ? -1 : 0}
              onClick={() => { if (!uploading) fileRef.current?.click(); }}
              onKeyDown={(e) => {
                if (!uploading && (e.key === "Enter" || e.key === " ")) {
                  e.preventDefault();
                  fileRef.current?.click();
                }
              }}
              className={`flex ${density === "compact" ? "h-20" : "h-28 sm:h-36"} flex-col items-center justify-center space-y-1 rounded-xl border-2 border-dashed border-zinc-700 bg-zinc-900/40 text-zinc-400 backdrop-blur-sm transition-all hover:border-brand-500/60 hover:bg-zinc-900/70 hover:text-zinc-200 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${uploading ? "opacity-60" : ""}`}
            >
              <IconPlus className={density === "compact" ? "h-4 w-4" : "h-5 w-5"} />
              <span className="font-mono text-3xs sm:text-2xs">
                {uploading
                  ? `Enviando ${uploadSent}/${uploadCount}`
                  : "Adicionar imagens"}
              </span>
              {uploading && uploadBatchInfo && (
                <span className="font-mono text-4xs text-zinc-500">
                  lote {uploadBatchInfo.batchIndex}/{uploadBatchInfo.batchCount}
                </span>
              )}
            </div>
            {uploading && (
              <button
                type="button"
                onClick={() => { uploadCancelledRef.current = true; }}
                className="absolute bottom-2 right-2 rounded-md border border-status-danger/30 bg-status-danger/[0.12] px-2 py-0.5 font-mono text-3xs text-rose-300 transition-colors hover:bg-status-danger/[0.20]"
              >
                Cancelar
              </button>
            )}
          </div>
        </div>
      )}

      {/* Sentinela do Scroll Infinito */}
      <div ref={sentinelRef} className="h-10 w-full flex items-center justify-center py-2">
        {loadingMore && (
          <div className="flex items-center gap-2 font-mono text-xs text-zinc-500">
            <Spinner className="size-3" />
            <span>Carregando mais amostras…</span>
          </div>
        )}
      </div>

      <FloatingSelectionBar
        selectedCount={selectedIds.size}
        totalInView={items.length}
        onSelectAll={handleSelectAll}
        onClearSelection={handleClearSelection}
        onBatchDelete={() => setBatchDeleteOpen(true)}
        onAutoLabel={() => setAutoLabelOpen(true)}
        onBatchEditClasses={() => setBatchEditClassesOpen(true)}
        busy={batchDeleteBusy}
      />

      <UploadFloatingDock
        uploading={uploading}
        uploadSent={uploadSent}
        uploadCount={uploadCount}
        uploadBatchInfo={uploadBatchInfo}
        lastResults={lastUploadResults}
        onCancel={() => { uploadCancelledRef.current = true; }}
        onOpenAudit={() => setAuditModalOpen(true)}
        onDismiss={() => setLastUploadResults(null)}
      />

      <UploadAuditModal
        open={auditModalOpen}
        results={lastUploadResults ?? []}
        onClose={() => setAuditModalOpen(false)}
      />

      <ImageQuickLookModal
        open={quickLookIndex !== null}
        dataset={dataset}
        items={items}
        currentIndex={quickLookIndex ?? 0}
        onClose={() => setQuickLookIndex(null)}
        onNavigate={(idx) => setQuickLookIndex(idx)}
        onDelete={(img) => setDeleting(img)}
        onEditImage={(img: ImageItem) => handleTileClick(img)}
        onCaptionUpdated={(imgId, text) => {
          setItems((prev) =>
            prev.map((it) => (it.id === imgId ? { ...it, caption: text } : it)),
          );
        }}
      />

      <ConfirmDialog
        open={batchDeleteOpen}
        title="Mover imagens selecionadas para a lixeira"
        body={
          <p>
            Você tem certeza de que deseja mover{" "}
            <strong className="font-mono text-zinc-100">{selectedIds.size} imagens</strong> para
            a lixeira? Elas poderão ser restauradas a qualquer momento.
          </p>
        }
        confirmLabel="Mover para a lixeira"
        danger
        busy={batchDeleteBusy}
        onConfirm={handleConfirmBatchDelete}
        onClose={() => {
          if (!batchDeleteBusy) setBatchDeleteOpen(false);
        }}
      />

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
      {trainOpen && dataset && (
        <TrainYoloModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          onClose={() => setTrainOpen(false)}
          onJobCreated={() => setTrainOpen(false)}
        />
      )}
      {autoTrackerOpen && dataset && (
        <AutoTrackerModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          onClose={() => setAutoTrackerOpen(false)}
          onJobCreated={() => setAutoTrackerOpen(false)}
        />
      )}
      {autoLabelOpen && dataset && (
        <AutoLabelModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          classes={dataset.classes ?? []}
          selectedImageIds={Array.from(selectedIds)}
          totalImagesCount={dataset.imagesCount}
          onClose={() => setAutoLabelOpen(false)}
          onJobCreated={() => setAutoLabelOpen(false)}
        />
      )}
      {batchEditClassesOpen && dataset && (
        <BatchEditClassesModal
          open
          datasetId={dataset.id}
          datasetClasses={dataset.classes ?? []}
          selectedImageIds={Array.from(selectedIds)}
          totalInView={items.length}
          onClose={() => setBatchEditClassesOpen(false)}
          onSuccess={() => {
            if (datasetId) {
              void load(datasetId, splitView, annotationFilter, selectedClassId, activeTag);
            }
          }}
          onClassesUpdated={(newClasses) => {
            setDataset({ ...dataset, classes: newClasses });
          }}
        />
      )}
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
