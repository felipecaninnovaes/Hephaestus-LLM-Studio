"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useRef, useState } from "react";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { getDataset } from "@/lib/datasets";
import {
  listImages,
  purgeTrash,
  restoreImage,
  softDeleteImage,
} from "@/lib/images";
import {
  getSearchStatus,
  searchByImage,
  searchDataset,
  triggerSearchIndex,
} from "@/lib/search";
import type {
  Dataset,
  ImageItem,
  SearchItem,
  SearchStatus,
  StudioClass,
} from "@/types/studio";
import type {
  GalleryAnnotationFilter,
  GalleryDensity,
  GallerySplitView,
} from "@/components/studio/GalleryOperateToolbar";

export const PAGE_LIMIT = 50;

export type GalleryView = "ativas" | "trash";

export interface UseDatasetGalleryOptions {
  datasetId?: string;
}

export function useDatasetGallery({ datasetId }: UseDatasetGalleryOptions) {
  const router = useRouter();

  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [items, setItems] = useState<ImageItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [view, setView] = useState<GalleryView>("ativas");
  const [splitView, setSplitView] = useState<GallerySplitView>("all");
  const [annotationFilter, setAnnotationFilter] =
    useState<GalleryAnnotationFilter>("all");
  const [density, setDensity] = useState<GalleryDensity>("normal");
  const [selectedClassId, setSelectedClassId] = useState<string | null>(null);

  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [quickLookIndex, setQuickLookIndex] = useState<number | null>(null);

  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const [trashTotal, setTrashTotal] = useState(0);

  const [deleting, setDeleting] = useState<ImageItem | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [purgeOpen, setPurgeOpen] = useState(false);
  const [purgeBusy, setPurgeBusy] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);

  const [batchDeleteBusy, setBatchDeleteBusy] = useState(false);
  const [batchDeleteOpen, setBatchDeleteOpen] = useState(false);

  // Search
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

  const pollRef = useRef<number | null>(null);
  const pollAbortRef = useRef<AbortController | null>(null);
  const searchTimerRef = useRef<number | null>(null);
  const lastSearchedRef = useRef<string | null>(null);

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
    if (
      datasetId &&
      !(searchMode === "semantic" && (activeQuery || similarFor))
    ) {
      void load(
        datasetId,
        splitView,
        annotationFilter,
        selectedClassId,
        activeTag,
      );
    }
  }, [
    datasetId,
    load,
    splitView,
    annotationFilter,
    selectedClassId,
    activeTag,
    searchMode,
    activeQuery,
    similarFor,
  ]);

  useEffect(() => {
    function handleDatasetUpdated(e: Event) {
      const ce = e as CustomEvent<{ datasetId?: string }>;
      if (datasetId && (!ce.detail || ce.detail.datasetId === datasetId)) {
        void load(
          datasetId,
          splitView,
          annotationFilter,
          selectedClassId,
          activeTag,
        );
      }
    }
    window.addEventListener("hephaestus:dataset-updated", handleDatasetUpdated);
    return () => {
      window.removeEventListener(
        "hephaestus:dataset-updated",
        handleDatasetUpdated,
      );
    };
  }, [datasetId, load, splitView, annotationFilter, selectedClassId, activeTag]);

  const stopSearchPolling = useCallback(() => {
    if (pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
    pollAbortRef.current?.abort();
    pollAbortRef.current = null;
  }, []);

  const startSearchPolling = useCallback(() => {
    if (pollRef.current !== null) return;
    const pollCtrl = new AbortController();
    pollAbortRef.current = pollCtrl;
    pollRef.current = window.setInterval(async () => {
      if (
        typeof document !== "undefined" &&
        document.visibilityState === "hidden"
      ) {
        return;
      }
      try {
        const next = await getSearchStatus(
          datasetId as string,
          pollCtrl.signal,
        );
        if (pollCtrl.signal.aborted) return;
        setSearchStatus(next);
        setStatusFailed(false);
        if (next.status !== "indexing") stopSearchPolling();
      } catch {
        if (pollCtrl.signal.aborted) return;
      }
    }, 2000);
  }, [datasetId, stopSearchPolling]);

  const clearSearch = useCallback(() => {
    setActiveQuery(null);
    setActiveTag(null);
    setSimilarFor(null);
    setResults([]);
    setSearchInput("");
  }, []);

  const handleTextSearch = useCallback(
    async (query: string, mode: "tag" | "semantic" = searchMode) => {
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
        showToast(
          err instanceof ApiError && err.message
            ? err.message
            : "Falha na busca.",
          "error",
        );
      } finally {
        setSearching(false);
      }
    },
    [datasetId, searchMode, clearSearch, router],
  );

  // Status do índice
  // biome-ignore lint/correctness/useExhaustiveDependencies: items.length é trigger intencional
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
    void fetchStatus();
    return () => {
      cancelled = true;
      ctrl.abort();
      stopSearchPolling();
    };
  }, [datasetId, items.length, startSearchPolling, stopSearchPolling]);

  // Debounced search
  useEffect(() => {
    if (searchTimerRef.current !== null) {
      window.clearTimeout(searchTimerRef.current);
      searchTimerRef.current = null;
    }
    const q = searchInput.trim();
    if (!q) {
      lastSearchedRef.current = `${searchMode}:`;
      if (searchMode === "semantic" && activeQuery) {
        clearSearch();
      } else if (searchMode === "tag" && activeTag) {
        setActiveTag(null);
      }
      return;
    }
    searchTimerRef.current = window.setTimeout(() => {
      const target = `${searchMode}:${q}`;
      if (lastSearchedRef.current === target) return;
      lastSearchedRef.current = target;
      void handleTextSearch(searchInput, searchMode);
    }, 400);
    return () => {
      if (searchTimerRef.current !== null) {
        window.clearTimeout(searchTimerRef.current);
        searchTimerRef.current = null;
      }
    };
  }, [
    searchInput,
    searchMode,
    activeQuery,
    activeTag,
    clearSearch,
    handleTextSearch,
  ]);

  const loadMore = useCallback(async () => {
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
  }, [
    datasetId,
    loadingMore,
    splitView,
    annotationFilter,
    selectedClassId,
    searchMode,
    activeTag,
    items.length,
    router,
  ]);

  // Infinite Scroll
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
  }, [loading, loadingMore, items.length, total, loadMore]);

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

  const isAllSelected =
    items.length > 0 && items.every((i) => selectedIds.has(i.id));

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
          // segue para a próxima
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

  const clearAllFilters = () => {
    clearSearch();
    setSelectedClassId(null);
  };

  const handleSimilarSearch = async (item: ImageItem) => {
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
      showToast("Falha na busca por similaridade.", "error");
    } finally {
      setSearching(false);
    }
  };

  const handleTriggerIndex = async () => {
    if (!datasetId || indexBusy) return;
    setIndexBusy(true);
    try {
      await triggerSearchIndex(datasetId);
      setSearchStatus((prev) =>
        prev ? { ...prev, status: "indexing" } : prev,
      );
      startSearchPolling();
      showToast("Indexação semântica iniciada.", "info");
    } catch {
      showToast("Falha ao iniciar indexação.", "error");
    } finally {
      setIndexBusy(false);
    }
  };

  const handleSoftDelete = async (item: ImageItem) => {
    if (!datasetId) return;
    setDeleteBusy(true);
    try {
      await softDeleteImage(datasetId, item.id);
      showToast("Imagem movida para a lixeira.", "success");
      setDeleting(null);
      await load(datasetId, splitView, annotationFilter);
    } catch {
      showToast("Falha ao mover imagem para a lixeira.", "error");
    } finally {
      setDeleteBusy(false);
    }
  };

  const handleRestore = async (id: string) => {
    if (!datasetId) return;
    setRestoringId(id);
    try {
      await restoreImage(datasetId, id);
      showToast("Imagem restaurada.", "success");
      await load(datasetId, splitView, annotationFilter);
    } catch {
      showToast("Falha ao restaurar imagem.", "error");
    } finally {
      setRestoringId(null);
    }
  };

  const handlePurgeTrash = async () => {
    if (!datasetId) return;
    setPurgeBusy(true);
    try {
      await purgeTrash(datasetId);
      showToast("Lixeira esvaziada com sucesso.", "success");
      await load(datasetId, splitView, annotationFilter);
    } catch {
      showToast("Falha ao esvaziar lixeira.", "error");
    } finally {
      setPurgeBusy(false);
    }
  };

  const handleClassesSaved = async (classes: StudioClass[]) => {
    setDataset((prev) => (prev ? { ...prev, classes } : prev));
    if (!datasetId) return;
    try {
      const fresh = await getDataset(datasetId);
      setDataset(fresh);
    } catch {
      // Falha silenciosa
    }
  };

  return {
    dataset,
    setDataset,
    items,
    setItems,
    total,
    trashTotal,
    loading,
    loadingMore,
    error,
    load,
    loadMore,
    sentinelRef,
    view,
    setView,
    splitView,
    setSplitView,
    annotationFilter,
    setAnnotationFilter,
    density,
    setDensity,
    selectedClassId,
    setSelectedClassId,
    handleSplitChange,
    handleAnnotationChange,
    clearAllFilters,
    searchMode,
    setSearchMode,
    searchInput,
    setSearchInput,
    activeTag,
    activeQuery,
    similarFor,
    results,
    searching,
    searchStatus,
    statusFailed,
    indexBusy,
    clearSearch,
    handleTextSearch,
    handleSimilarSearch,
    handleTriggerIndex,
    selectionMode,
    setSelectionMode,
    selectedIds,
    setSelectedIds,
    handleSelectToggle,
    handleSelectAll,
    handleClearSelection,
    isAllSelected,
    batchDeleteOpen,
    setBatchDeleteOpen,
    batchDeleteBusy,
    handleConfirmBatchDelete,
    deleting,
    setDeleting,
    deleteBusy,
    handleSoftDelete,
    restoringId,
    handleRestore,
    purgeOpen,
    setPurgeOpen,
    purgeBusy,
    handlePurgeTrash,
    quickLookIndex,
    setQuickLookIndex,
    handleOpenQuickLook,
    handleClassesSaved,
  };
}
