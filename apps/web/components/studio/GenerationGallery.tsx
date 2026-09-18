"use client";

import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  IconCheck,
  IconCopy,
  IconDownload,
  IconImage,
  IconSliders,
  IconTrash,
  IconX,
  IconZoomIn,
} from "@/components/icons";
import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  GlassCard,
  Modal,
  TruncatedText,
  showToast,
} from "@/components/ui";
import {
  listGenerations,
  deleteGenerations,
  exportGenerations,
  getGenerationDataUrl,
} from "@/lib/generations";
import { GERACAO_BROADCAST_CHANNEL, GERACAO_COMPLETED_KEY, generationConfigsJson, geracaoFormFromGeneration, publishGeracaoForm, publishGeracaoInitSource } from "@/lib/geracao-storage";
import { copyToClipboard } from "@/lib/clipboard";
import type { Generation } from "@/types/studio";
import CompareSlider from "./CompareSlider";

/* ── Constantes ── */

const PAGE_LIMIT = 50;

/** Limite defensivo por call ao paginar "carregar todas" (clamp 1..200 do backend). */
const LOAD_ALL_LIMIT = 200;
/** Teto de páginas por ação "carregar todas" (20 × 200 = 4000 itens máx.). */
const LOAD_ALL_MAX_PAGES = 20;
/** Teto do backend por call de delete/export (≤100 ids — handlers.rs:276,303). */
const BACKEND_BATCH_LIMIT = 100;

/** Chaves estáveis dos 12 placeholders de skeleton (lista estática — nunca reordena). */
const GALLERY_SKELETON_KEYS = [
  "gallery-skeleton-01",
  "gallery-skeleton-02",
  "gallery-skeleton-03",
  "gallery-skeleton-04",
  "gallery-skeleton-05",
  "gallery-skeleton-06",
  "gallery-skeleton-07",
  "gallery-skeleton-08",
  "gallery-skeleton-09",
  "gallery-skeleton-10",
  "gallery-skeleton-11",
  "gallery-skeleton-12",
];

/* ═══════════════════════════════════════════════════════════════════
   GenerationGallery — galeria persistente de imagens geradas (G.8)
   Grade responsiva com seleção, delete/export em lote, comparador.
   ═══════════════════════════════════════════════════════════════════ */

export default function GenerationGallery() {
  /* ── Data ── */
  const [items, setItems] = useState<Generation[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /* ── Selection ── */
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  /** Âncora da seleção por faixa (ID do item em `items`, ordem visual created_at DESC).
      Âncora por ID (não índice): o prepend do refreshGallery invalida índices.
      Atualizada SOMENTE em cliques sem Shift; Shift+clique nunca move a âncora. */
  const anchorIdRef = useRef<string | null>(null);
  /** Paginação "carregar todas" em andamento (desabilita toolbar). */
  const [loadingAll, setLoadingAll] = useState(false);

  /* ── Lightbox ── */
  const [lightboxItem, setLightboxItem] = useState<Generation | null>(null);

  /* ── Compare ── */
  const [compareOpen, setCompareOpen] = useState(false);
  const [compareA, setCompareA] = useState<Generation | null>(null);
  const [compareB, setCompareB] = useState<Generation | null>(null);

  /* ── Delete confirm ── */
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteBusy, setDeleteBusy] = useState(false);

  /* ── Export busy ── */
  const [exportBusy, setExportBusy] = useState(false);

  /* ── Infinite scroll sentinel ── */
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  /** Guarda anti-setState-em-unmount para o loop paginado de loadAll. */
  const mountedRef = useRef(true);

  useEffect(() => () => {
    mountedRef.current = false;
  }, []);

  /* ── Derived ── */
  const selectedArray = useMemo(() => Array.from(selectedIds), [selectedIds]);
  const selectedCount = selectedArray.length;
  /** Todas as CARREGADAS selecionadas (honesto quando total > items.length). */
  const allLoadedSelected = useMemo(
    () => items.length > 0 && items.every((i) => selectedIds.has(i.id)),
    [items, selectedIds],
  );
  const hasMoreToLoad = items.length < total;

  /* ── Load initial page ── */
  const loadInitial = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await listGenerations({ limit: PAGE_LIMIT, offset: 0, deleted: false });
      setItems(res.items);
      setTotal(res.total);
    } catch {
      setError("Falha ao carregar a galeria.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadInitial();
  }, [loadInitial]);

  /* ── Load more (infinite scroll) ── */
  const loadMore = useCallback(async () => {
    /* Trava contra loadAll/refresh (ref espelho: sem re-criar o callback). */
    if (loadingMore || loadingAllRef.current || items.length >= total) return;
    setLoadingMore(true);
    try {
      const res = await listGenerations({
        limit: PAGE_LIMIT,
        offset: items.length,
        deleted: false,
      });
      setItems((prev) => [...prev, ...res.items]);
      setTotal(res.total);
      /* Append desloca faixas de Shift: invalida a âncora. */
      anchorIdRef.current = null;
    } catch {
      showToast("Falha ao carregar mais gerações.", "error");
    } finally {
      setLoadingMore(false);
    }
  }, [loadingMore, items.length, total]);

  /* ── IntersectionObserver for infinite scroll ── */
  useEffect(() => {
    const el = sentinelRef.current;
    if (!el) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (
          entries[0]?.isIntersecting &&
          !loading &&
          !loadingMore &&
          !loadingAllRef.current &&
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

  /* ── Refresh silencioso (Slice F2/002) ──
     Recarrega o intervalo já carregado (limit = max(PAGE_LIMIT, N atual),
     offset 0) para que imagens novas façam prepend sem resetar scroll
     (não toca em scrollTop; o container mantém a posição) e sem perder
     paginação/infinite-scroll. Seleção preservada por id existente. Sem
     setInterval permanente: o gatilho é evento, não polling. */
  const refreshingRef = useRef(false);
  const lastRefreshRef = useRef(0);
  const itemsLengthRef = useRef(0);
  const loadingRef = useRef(false);
  const loadingMoreRef = useRef(false);
  /** Espelho ref de `loadingAll` (loadMore/observer/refresh o leem sem re-subscrever). */
  const loadingAllRef = useRef(false);
  loadingRef.current = loading;
  loadingMoreRef.current = loadingMore;
  loadingAllRef.current = loadingAll;

  useEffect(() => {
    itemsLengthRef.current = items.length;
  }, [items.length]);

  const refreshGallery = useCallback(async (force = false) => {
    /* Cooldown 5s apenas para eventos passivos; eventos explícitos de conclusão (force=true) bypassam. */
    const now = Date.now();
    if (refreshingRef.current) return;
    if (loadingRef.current || loadingMoreRef.current || loadingAllRef.current) return;
    if (!force && now - lastRefreshRef.current < 5000) return;
    refreshingRef.current = true;
    lastRefreshRef.current = now;
    try {
      // openapi clamp 1..200; sem teto, lista truncaria e podaria seleção.
      // TODO(filtros): propagar baseModel/quantization ao refresh se a UI de filtros existir
      const limit = Math.min(200, Math.max(PAGE_LIMIT, itemsLengthRef.current));
      const res = await listGenerations({ limit, offset: 0, deleted: false });
      setItems(res.items);
      setTotal(res.total);
      setSelectedIds((prev) => {
        if (prev.size === 0) return prev;
        const alive = new Set(res.items.map((i) => i.id));
        let dropped = false;
        const next = new Set<string>();
        for (const id of prev) {
          if (alive.has(id)) next.add(id);
          else dropped = true;
        }
        return dropped ? next : prev;
      });
      /* Lista substituída (prepend reordena): a âncora de Shift pode ter
         sumido ou mudado de posição — invalida. */
      anchorIdRef.current = null;
    } catch {
      /* refresh silencioso: sem toast para não spammar aba em background */
    } finally {
      refreshingRef.current = false;
    }
  }, []);

  /* ── Canal primário cross-tab: BroadcastChannel + evento `storage` ── */
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === GERACAO_COMPLETED_KEY) void refreshGallery(true);
    };
    window.addEventListener("storage", onStorage);

    let bc: BroadcastChannel | null = null;
    if (typeof BroadcastChannel !== "undefined") {
      bc = new BroadcastChannel(GERACAO_BROADCAST_CHANNEL);
      bc.onmessage = (ev) => {
        if (ev.data?.type === "generation_completed") {
          void refreshGallery(true);
        }
      };
    }

    return () => {
      window.removeEventListener("storage", onStorage);
      if (bc) bc.close();
    };
  }, [refreshGallery]);

  /* ── Mesma-aba: `storage` não dispara na aba de origem; cobre via
     foco/visibilidade. Mount já recarrega via loadInitial. ── */
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") void refreshGallery();
    };
    const onFocus = () => {
      void refreshGallery();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", onFocus);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", onFocus);
    };
  }, [refreshGallery]);

  /* ── Heartbeat/poll de galeria quando aba visível (atualização autônoma a cada 12s) ── */
  useEffect(() => {
    const interval = setInterval(() => {
      if (document.visibilityState === "visible") {
        void refreshGallery(false);
      }
    }, 12000);
    return () => clearInterval(interval);
  }, [refreshGallery]);

  /* ── Selection handlers (Slice F3/bug-008) ──
     Interação escolhida:
     - Clique simples = toggle do item + move a âncora para o índice clicado.
     - Shift+clique = ADICIONA (nunca remove) a faixa [âncora..atual] na ordem
       visual de `items`; sem âncora prévia, comporta-se como clique simples.
     - "Selecionar todas" = todas as CARREGADAS; hint "N de M" denuncia o
       restante não carregado; "Carregar todas" pagina (200/call, máx. 20
       páginas) até esgotar — sem loop infinito: para em página vazia,
       total atingido ou teto de páginas. */
  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const handleCardClick = useCallback(
    (id: string, index: number, shiftKey: boolean) => {
      /* Âncora por ID: resolve o índice atual (o prepend do refreshGallery
         invalida índices guardados; ID sobrevive à reordenação). */
      const anchorIdx =
        anchorIdRef.current !== null
          ? items.findIndex((i) => i.id === anchorIdRef.current)
          : -1;
      if (shiftKey && anchorIdx >= 0) {
        const lo = Math.max(0, Math.min(anchorIdx, index));
        const hi = Math.min(items.length - 1, Math.max(anchorIdx, index));
        const rangeIds = items.slice(lo, hi + 1).map((i) => i.id);
        setSelectedIds((prev) => {
          const next = new Set(prev);
          for (const rid of rangeIds) next.add(rid);
          return next;
        });
        return;
      }
      /* Sem âncora ou âncora sumida da lista: clique simples + nova âncora. */
      anchorIdRef.current = id;
      toggleSelect(id);
    },
    [items, toggleSelect],
  );

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(items.map((i) => i.id)));
    /* Âncora coerente: início da faixa (tudo já selecionado; Shift+clique vira no-op). */
    anchorIdRef.current = items[0]?.id ?? null;
  }, [items]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
    anchorIdRef.current = null;
  }, []);

  /* ── Carregar todas (pagina até esgotar, com travas anti-loop) ── */
  const loadAll = useCallback(async () => {
    /* Trava contra refreshGallery/loadMore (refs: sem re-criar o callback). */
    if (loadingAll || loading || loadingMore) return;
    if (refreshingRef.current || loadingRef.current) return;
    if (items.length >= total) return;
    setLoadingAll(true);
    try {
      let accumulated: Generation[] = [];
      let pages = 0;
      let expectedTotal = total;
      while (pages < LOAD_ALL_MAX_PAGES) {
        if (!mountedRef.current) return;
        const offset = items.length + accumulated.length;
        if (offset >= expectedTotal) break;
        const res = await listGenerations({
          limit: LOAD_ALL_LIMIT,
          offset,
          deleted: false,
        });
        if (!mountedRef.current) return;
        expectedTotal = res.total;
        if (res.items.length === 0) break;
        accumulated = [...accumulated, ...res.items];
        pages += 1;
        if (items.length + accumulated.length >= expectedTotal) break;
      }
      if (!mountedRef.current) return;
      if (accumulated.length > 0) {
        setItems((prev) => [...prev, ...accumulated]);
        setTotal(expectedTotal);
        /* Append em massa desloca faixas de Shift: invalida a âncora. */
        anchorIdRef.current = null;
      }
    } catch {
      if (mountedRef.current) showToast("Falha ao carregar todas as gerações.", "error");
    } finally {
      if (mountedRef.current) setLoadingAll(false);
    }
  }, [loadingAll, loading, loadingMore, items.length, total]);

  /* ── Delete handler: lotes sequenciais de 100, um único toast ao final. ── */
  const handleDelete = useCallback(async () => {
    if (selectedCount === 0) return;
    setDeleteBusy(true);
    try {
      let deleted = 0;
      let partial = false;
      for (let i = 0; i < selectedArray.length; i += BACKEND_BATCH_LIMIT) {
        try {
          await deleteGenerations(selectedArray.slice(i, i + BACKEND_BATCH_LIMIT));
          deleted += Math.min(BACKEND_BATCH_LIMIT, selectedArray.length - i);
        } catch {
          /* Para no 1º lote com falha; o restante mantém a seleção p/ retry. */
          partial = true;
          break;
        }
      }
      if (partial) {
        showToast(`Exclusão parcial: ${deleted} de ${selectedCount} imagens excluídas.`, "error");
        /* Refetch honesto bypassando o cooldown do refresh silencioso. */
        lastRefreshRef.current = 0;
        void refreshGallery();
      } else {
        const deletedIds = new Set(selectedArray);
        setItems((prev) => prev.filter((i) => !deletedIds.has(i.id)));
        /* DELETE retorna 204 sem corpo (sem res.total) → clamp local honesto. */
        setTotal((prev) => Math.max(0, prev - deleted));
        setSelectedIds(new Set());
        anchorIdRef.current = null;
        showToast(`${deleted} imagens excluídas.`, "success");
      }
      setDeleteOpen(false);
    } catch {
      showToast("Falha ao excluir gerações.", "error");
    } finally {
      setDeleteBusy(false);
    }
  }, [selectedArray, selectedCount, refreshGallery]);

  /* ── Export handler: 1 call; gate honesto acima de 100 (sem chunk silencioso). ── */
  const handleExport = useCallback(async () => {
    if (selectedCount === 0) return;
    if (selectedCount > BACKEND_BATCH_LIMIT) {
      showToast("Exportação limitada a 100 imagens por vez.", "error");
      return;
    }
    setExportBusy(true);
    try {
      await exportGenerations(selectedArray);
      showToast("Exportação concluída!", "success");
    } catch {
      showToast("Falha ao exportar gerações.", "error");
    } finally {
      setExportBusy(false);
    }
  }, [selectedArray, selectedCount]);

  /* ── Compare handler ── */
  const handleCompare = useCallback(() => {
    if (selectedCount !== 2) return;
    const [a, b] = selectedArray;
    const genA = items.find((i) => i.id === a);
    const genB = items.find((i) => i.id === b);
    if (genA && genB) {
      setCompareA(genA);
      setCompareB(genB);
      setCompareOpen(true);
    }
  }, [selectedArray, selectedCount, items]);

  /* ── Download individual ── */
  const handleDownloadSingle = useCallback(async (gen: Generation) => {
    try {
      const url = gen.url || getGenerationDataUrl(gen.id);
      const res = await fetch(url);
      const blob = await res.blob();
      const objectUrl = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = objectUrl;
      a.download = gen.filename || `geracao-${gen.seed}.png`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
      showToast("Download concluído!", "success");
    } catch {
      showToast("Falha ao baixar imagem.", "error");
    }
  }, []);

  /* ── Image URL helper ── */
  const getImageUrl = useCallback((gen: Generation): string => {
    return gen.thumbUrl || gen.url || getGenerationDataUrl(gen.id);
  }, []);

  /* ── Copiar configs (Slice F4/007): JSON legível p/ reprodução.
     Ação explícita — NUNCA sobrecarrega o clique do card (toggle de
     seleção). Via copyToClipboard (fallback execCommand p/ HTTP em LAN). ── */
  const handleCopyConfigs = useCallback(async (gen: Generation) => {
    const ok = await copyToClipboard(generationConfigsJson(gen));
    showToast(ok ? "Configs copiadas!" : "Falha ao copiar configs.", ok ? "success" : "error");
  }, []);

  /* ── Copiar prompt (bônus trivial) ── */
  const handleCopyPrompt = useCallback(async (gen: Generation) => {
    if (gen.prompt.trim().length === 0) {
      showToast("Prompt vazio — nada para copiar.", "error");
      return;
    }
    const ok = await copyToClipboard(gen.prompt);
    showToast(ok ? "Prompt copiado!" : "Falha ao copiar prompt.", ok ? "success" : "error");
  }, []);

  /* ── Usar configs no gerador: grava `geracao:form:v1` + evento canônico
     mesma-aba + volta p/ a aba Gerar (o Panel hidrata no mount; se já
     montado, o listener re-hidrata ao vivo). Resíduos não-reaplicáveis
     (LoRA sem UUID, checkpoint custom sem UUID) geram aviso explícito —
     o Toast só tem success|error|info, sem variante warning: usa "error". ── */
  const handleUseConfigs = useCallback((gen: Generation) => {
    const { form, warnings } = geracaoFormFromGeneration(gen);
    publishGeracaoForm(form);
    window.dispatchEvent(new CustomEvent("hephaestus:switch-tab", { detail: "gerar" }));
    showToast("Configs aplicadas no gerador.", "success");
    if (warnings.length > 0) showToast(warnings.join(" "), "error");
  }, []);

  const getFullImageUrl = useCallback((gen: Generation): string => {
    return gen.url || getGenerationDataUrl(gen.id);
  }, []);

  /* ── Usar como imagem inicial img2img (fatia feat/img2img, S5) ──
     Só gerações concluídas com imagem chegam aqui: a galeria lista
     somente gerações persistidas (concluídas) e o lightbox abre para um
     item concreto com imagem resolvível. Publica `geracao:initSource` +
     evento `heph:init-source` (o Panel consome no mount/evento/storage)
     e volta p/ a aba Gerar. Sem toast aqui — o Panel confirma ao receber
     ("Imagem da galeria carregada como entrada."), evitando duplicata. ── */
  const handleUseAsInit = useCallback((gen: Generation) => {
    publishGeracaoInitSource(gen.id);
    setLightboxItem(null);
    window.dispatchEvent(new CustomEvent("hephaestus:switch-tab", { detail: "gerar" }));
  }, []);

  /* ═══════════════════════════════════════════════════════════════════
     RENDER
     ═══════════════════════════════════════════════════════════════════ */

  /* ── Loading state ── */
  if (loading) {
    return (
      <div className="p-4 md:p-6">
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-3">
          {GALLERY_SKELETON_KEYS.map((skeletonKey) => (
            <div
              key={skeletonKey}
              className="aspect-square rounded-xl bg-zinc-900/60 border border-white/5 animate-pulse"
            />
          ))}
        </div>
      </div>
    );
  }

  /* ── Error state ── */
  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <EmptyState
          icon={<IconImage className="size-8 text-rose-400" />}
          title="Erro ao carregar galeria"
          description={error}
          actionLabel="Tentar novamente"
          onAction={loadInitial}
        />
      </div>
    );
  }

  /* ── Empty state ── */
  if (items.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <EmptyState
          icon={<IconImage className="size-8 text-brand-400" />}
          title="Nenhuma imagem gerada ainda"
          description="Gere imagens na aba Gerar para vê-las aqui."
          actionLabel="Ir para Gerar"
          onAction={() => {
            /* parent switches tab via URL or state — sibling prop not available;
               dispatch custom event for parent to catch */
            window.dispatchEvent(new CustomEvent("hephaestus:switch-tab", { detail: "gerar" }));
          }}
        />
      </div>
    );
  }

  return (
    <div className="flex min-h-full flex-col lg:h-full lg:min-h-0 lg:overflow-hidden">
      {/* ── Header ── */}
      <div className="shrink-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-1 px-4 py-3 md:px-6 border-b border-white/5">
        <div className="flex items-baseline gap-x-3 gap-y-0.5 flex-wrap">
          <h2 className="font-display text-sm font-bold text-white">Galeria</h2>
          <span className="font-mono text-2xs text-zinc-400 shrink-0">
            {total} {total === 1 ? "geração" : "gerações"}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          {selectedCount > 0 && (
            <span className="font-mono text-2xs text-brand-400">
              {selectedCount} selecionada{selectedCount > 1 ? "s" : ""}
            </span>
          )}
          {/* Toolbar de seleção (bug-008): visível quando há itens carregados */}
          {items.length > 0 && (
            <div className="flex items-center gap-2" role="toolbar" aria-label="Seleção da galeria">
              {allLoadedSelected ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={clearSelection}
                  aria-label="Limpar seleção da galeria"
                >
                  Limpar seleção
                </Button>
              ) : (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={selectAll}
                  aria-label="Selecionar todas as imagens carregadas"
                >
                  Selecionar todas ({items.length})
                </Button>
              )}
              {hasMoreToLoad ? (
                <span className="flex items-center gap-2">
                  <span
                    className="font-mono text-2xs text-zinc-500"
                    aria-live="polite"
                    title="A seleção cobre apenas os itens já carregados na grade"
                  >
                    {items.length} de {total}
                  </span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => void loadAll()}
                    disabled={loadingAll || loading || loadingMore}
                    aria-label={`Carregar todas as ${total} gerações para seleção completa`}
                  >
                    {loadingAll ? "Carregando…" : "Carregar todas"}
                  </Button>
                </span>
              ) : null}
            </div>
          )}
        </div>
      </div>

      {/* ── Grid ── */}
      <div className="flex-1 overflow-visible p-4 md:p-6 lg:min-h-0 lg:overflow-y-auto">
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-3">
          {items.map((gen, index) => {
            const isSelected = selectedIds.has(gen.id);
            return (
              <div
                key={gen.id}
                className="group relative aspect-square rounded-xl overflow-hidden border border-white/5 bg-zinc-900/60 transition-all hover:border-brand-500/30 hover:shadow-lg hover:shadow-brand-500/5"
              >
                {/* eslint-disable-next-line @next/next/no-img-element */}
                <img
                  src={getImageUrl(gen)}
                  alt={gen.prompt}
                  loading="lazy"
                  decoding="async"
                  className="absolute inset-0 w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
                  style={{ contentVisibility: "auto" }}
                />

                {/* Overlay gradiente */}
                <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-black/10 to-black/30 pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity" />

                {/* Seed badge — canto superior direito */}
                <div className="absolute top-2 right-2 z-10">
                  <Badge variant="mono" className="bg-black/60 backdrop-blur-md border-white/15">
                    seed {gen.seed}
                  </Badge>
                </div>

                {/* Checkbox — canto superior esquerdo */}
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleCardClick(gen.id, index, e.shiftKey);
                  }}
                  aria-label={isSelected ? `Desselecionar geração seed ${gen.seed}` : `Selecionar geração seed ${gen.seed}`}
                  aria-pressed={isSelected}
                  className={`absolute top-2 left-2 z-10 flex size-10 items-center justify-center rounded-lg border transition-all cursor-pointer ${
                    isSelected
                      ? "border-brand-500 bg-brand-500/20 text-brand-300"
                      : "border-white/20 bg-black/40 text-transparent opacity-0 group-hover:opacity-100 hover:border-white/40 hover:text-zinc-300"
                  }`}
                >
                  <IconCheck className="size-4" />
                </button>

                {/* Prompt truncado — hover */}
                <div className="absolute bottom-0 left-0 right-0 z-10 p-2 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none">
                  <TruncatedText
                    text={gen.prompt}
                    lines={2}
                    as="p"
                    className="text-3xs text-zinc-200 leading-tight"
                  />
                </div>

                {/* Ações do card — hover (copiar NÃO toca na seleção) */}
                <div className="absolute bottom-2 right-2 z-10 flex gap-1.5 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity">
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      void handleCopyConfigs(gen);
                    }}
                    aria-label={`Copiar configs da geração seed ${gen.seed} em JSON`}
                    title="Copiar configs (JSON)"
                    className="flex size-10 items-center justify-center rounded-lg bg-black/60 border border-white/20 text-zinc-200 hover:bg-black/80 backdrop-blur-md cursor-pointer"
                  >
                    <IconCopy className="size-4" />
                  </button>
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      setLightboxItem(gen);
                    }}
                    aria-label="Ampliar geração"
                    className="flex size-10 items-center justify-center rounded-lg bg-black/60 border border-white/20 text-zinc-200 hover:bg-black/80 backdrop-blur-md cursor-pointer"
                  >
                    <IconZoomIn className="size-4" />
                  </button>
                </div>

                {/* Click no card = toggle | Shift+clique = faixa âncora..atual */}
                <button
                  type="button"
                  onClick={(e) => handleCardClick(gen.id, index, e.shiftKey)}
                  className="absolute inset-0 z-[5] cursor-pointer"
                  aria-label={isSelected ? "Desselecionar (Shift+clique seleciona a faixa)" : "Selecionar (Shift+clique seleciona a faixa)"}
                  aria-pressed={isSelected}
                />
              </div>
            );
          })}
        </div>

        {/* ── Infinite scroll sentinel ── */}
        <div ref={sentinelRef} className="h-10 w-full flex items-center justify-center py-2">
          {loadingMore && (
            <div className="flex items-center gap-2 text-xs text-zinc-400 font-mono">
              <div className="size-3 rounded-full border-2 border-brand-500/40 border-t-brand-400 animate-spin" />
              Carregando mais…
            </div>
          )}
          {items.length >= total && items.length > 0 && (
            <span className="text-3xs font-mono text-zinc-600">
              {total} {total === 1 ? "geração" : "gerações"} no total
            </span>
          )}
        </div>
      </div>

      {/* ── Floating Selection Bar ── */}
      {selectedCount > 0 && (
        <section
          aria-label="Ações para gerações selecionadas"
          className="fixed bottom-6 left-1/2 -translate-x-1/2 z-40 flex items-center overflow-x-auto no-scrollbar space-x-3 rounded-2xl border border-brand-500/30 bg-zinc-950/95 px-4 py-2.5 shadow-2xl backdrop-blur-xl animate-in fade-in slide-in-from-bottom-5 max-w-[calc(100vw-2rem)]"
        >
          {/* Hairline zenital */}
          <div
            className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-brand-400/50 to-transparent"
            aria-hidden="true"
          />

          <div className="flex items-center space-x-2 border-r border-zinc-800 pr-3">
            <span className="flex size-6 items-center justify-center rounded-full bg-brand-500/20 text-brand-300 font-mono text-xs font-semibold">
              {selectedCount}
            </span>
            <span className="font-mono text-xs text-zinc-300 hidden sm:inline">
              {selectedCount === 1 ? "selecionada" : "selecionadas"}
            </span>
          </div>

          <div className="flex items-center space-x-2">
            <button
              type="button"
              onClick={allLoadedSelected ? clearSelection : selectAll}
              aria-label={allLoadedSelected ? `Limpar seleção (${selectedCount} selecionadas)` : `Selecionar todas as ${items.length} gerações carregadas`}
              className="flex items-center space-x-1.5 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1 font-mono text-xs text-zinc-300 transition-colors hover:bg-white/10 hover:text-white cursor-pointer"
            >
              <IconCheck className="size-3.5 text-brand-400" />
              <span className="hidden sm:inline">
                {allLoadedSelected ? "Limpar seleção" : "Selecionar todas"}
              </span>
            </button>

            <button
              type="button"
              onClick={handleCompare}
              disabled={selectedCount !== 2}
              className="flex items-center space-x-1.5 rounded-lg border border-cyan-500/40 bg-cyan-500/15 px-3 py-1 font-mono text-xs font-semibold text-cyan-300 transition-colors hover:bg-cyan-500/25 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
            >
              <span>Comparar (2)</span>
            </button>

            <button
              type="button"
              onClick={handleExport}
              disabled={exportBusy}
              className="flex items-center space-x-1.5 rounded-lg border border-brand-500/40 bg-brand-500/15 px-3 py-1 font-mono text-xs font-semibold text-brand-300 transition-colors hover:bg-brand-500/25 disabled:opacity-60 cursor-pointer"
            >
              <IconDownload className="size-3.5 text-brand-400" />
              <span>{exportBusy ? "Exportando…" : `Exportar (${selectedCount})`}</span>
            </button>

            <button
              type="button"
              onClick={() => setDeleteOpen(true)}
              className="flex items-center space-x-1.5 rounded-lg border border-rose-500/40 bg-rose-500/15 px-3 py-1 font-mono text-xs font-semibold text-rose-300 transition-colors hover:bg-rose-500/25 cursor-pointer"
            >
              <IconTrash className="size-3.5 text-rose-400" />
              <span className="hidden sm:inline">Excluir ({selectedCount})</span>
              <span className="sm:hidden">{selectedCount}</span>
            </button>

            <button
              type="button"
              onClick={clearSelection}
              aria-label="Cancelar seleção"
              title="Cancelar seleção"
              className="rounded-lg p-1 text-zinc-400 hover:bg-white/5 hover:text-white transition-colors cursor-pointer"
            >
              <IconX className="size-4" />
            </button>
          </div>
        </section>
      )}

      {/* ── Delete Confirm Dialog ── */}
      <ConfirmDialog
        open={deleteOpen}
        title="Excluir gerações"
        body={`Tem certeza que deseja excluir ${selectedCount} geração(ões)? Esta ação não pode ser desfeita.`}
        confirmLabel="Excluir"
        danger
        busy={deleteBusy}
        onConfirm={handleDelete}
        onClose={() => setDeleteOpen(false)}
      />

      {/* ── Lightbox Modal ── */}
      {lightboxItem && (
        <Modal
          open={!!lightboxItem}
          onClose={() => setLightboxItem(null)}
          title={`Seed ${lightboxItem.seed}`}
          maxWidth="xl"
          bodyClassName="p-0"
          headerRight={
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => handleDownloadSingle(lightboxItem)}
            >
              <IconDownload className="size-3.5" />
              <span className="ml-1">Baixar</span>
            </Button>
          }
        >
          <div className="flex flex-col gap-4">
            {/* Imagem */}
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={getFullImageUrl(lightboxItem)}
              alt={lightboxItem.prompt}
              className="w-full rounded-lg object-contain max-h-[60vh]"
            />

            {/* Metadados */}
            <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-xs px-1">
              <MetaRow label="Prompt" value={lightboxItem.prompt} />
              {lightboxItem.negativePrompt && (
                <MetaRow label="Negative" value={lightboxItem.negativePrompt} />
              )}
              <MetaRow label="Dimensões" value={`${lightboxItem.width}×${lightboxItem.height}`} mono />
              <MetaRow label="Seed" value={String(lightboxItem.seed)} mono />
              <MetaRow label="Base Model" value={String(lightboxItem.params?.base_model || lightboxItem.params?.baseModel || "—")} />
              {lightboxItem.params?.custom_model_id ? (
                <MetaRow label="Custom Model" value={String(lightboxItem.params.custom_model_id)} />
              ) : null}
              {lightboxItem.params?.text_encoder_model_id ? (
                <MetaRow label="Text Encoder" value={String(lightboxItem.params.text_encoder_model_id)} />
              ) : null}
              {lightboxItem.params?.steps ? (
                <MetaRow label="Steps" value={String(lightboxItem.params.steps)} mono />
              ) : null}
              {lightboxItem.params?.guidance_scale != null ? (
                <MetaRow label="CFG" value={String(lightboxItem.params.guidance_scale)} mono />
              ) : null}
              {lightboxItem.params?.quantization ? (
                <MetaRow label="Quantização" value={String(lightboxItem.params.quantization)} />
              ) : null}
              {lightboxItem.params?.sampler ? (
                <MetaRow label="Sampler" value={String(lightboxItem.params.sampler)} mono />
              ) : null}
              {lightboxItem.params?.upscale != null && typeof lightboxItem.params.upscale === "object" && "scale" in lightboxItem.params.upscale ? (
                <MetaRow
                  label="Upscale"
                  value={`Real-ESRGAN 4x · ${String(lightboxItem.params.upscale.scale)}x`}
                  mono
                />
              ) : null}
              {lightboxItem.params?.distilled != null ? (
                <MetaRow label="Destilado" value={lightboxItem.params.distilled ? "Sim" : "Não"} />
              ) : null}
              {lightboxItem.params?.loras && Array.isArray(lightboxItem.params.loras) && lightboxItem.params.loras.length > 0 ? (
                <MetaRow
                  label="LoRAs"
                  value={`${lightboxItem.params.loras.length} adaptador(es)`}
                />
              ) : null}
              <MetaRow label="Criado em" value={new Date(lightboxItem.createdAt).toLocaleString("pt-BR")} />
            </div>

            {/* Ações F4/007 — copiar configs/prompt, aplicar no gerador */}
            <div className="flex flex-wrap gap-2 px-1 pb-1">
              <Button
                type="button"
                variant="primary"
                size="sm"
                onClick={() => handleUseConfigs(lightboxItem)}
                aria-label={`Usar configs da geração seed ${lightboxItem.seed} no gerador`}
              >
                <IconSliders className="size-3.5" />
                <span className="ml-1">Usar estas configs</span>
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => handleUseAsInit(lightboxItem)}
                aria-label={`Usar geração seed ${lightboxItem.seed} como imagem inicial do img2img`}
                title="Carrega esta imagem como entrada do img2img na aba Gerar"
              >
                <IconImage className="size-3.5" />
                <span className="ml-1">Usar como imagem inicial</span>
              </Button>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => void handleCopyConfigs(lightboxItem)}
                aria-label={`Copiar configs da geração seed ${lightboxItem.seed} em JSON`}
              >
                <IconCopy className="size-3.5" />
                <span className="ml-1">Copiar configs</span>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => void handleCopyPrompt(lightboxItem)}
                aria-label={`Copiar prompt da geração seed ${lightboxItem.seed}`}
              >
                Copiar prompt
              </Button>
            </div>
          </div>
        </Modal>
      )}

      {/* ── Compare Slider ── */}
      {compareOpen && compareA && compareB && (
        <CompareSlider
          open={compareOpen}
          onClose={() => setCompareOpen(false)}
          imageA={compareA}
          imageB={compareB}
          getFullImageUrl={getFullImageUrl}
        />
      )}
    </div>
  );
}

/* ── Sub-componentes ── */

function MetaRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-mono text-3xs uppercase tracking-caps text-zinc-500">
        {label}
      </span>
      <TruncatedText
        text={value}
        lines={3}
        as="span"
        className={`text-zinc-200 ${mono ? "font-mono" : ""}`}
      />
    </div>
  );
}
