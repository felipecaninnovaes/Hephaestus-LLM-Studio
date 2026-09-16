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
  IconDownload,
  IconImage,
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
import type { Generation } from "@/types/studio";
import CompareSlider from "./CompareSlider";

/* ── Constantes ── */

const PAGE_LIMIT = 50;

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

  /* ── Derived ── */
  const selectedArray = useMemo(() => Array.from(selectedIds), [selectedIds]);
  const selectedCount = selectedArray.length;

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
    if (loadingMore || items.length >= total) return;
    setLoadingMore(true);
    try {
      const res = await listGenerations({
        limit: PAGE_LIMIT,
        offset: items.length,
        deleted: false,
      });
      setItems((prev) => [...prev, ...res.items]);
      setTotal(res.total);
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

  /* ── Selection handlers ── */
  const toggleSelect = useCallback((id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedIds(new Set(items.map((i) => i.id)));
  }, [items]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
  }, []);

  /* ── Delete handler ── */
  const handleDelete = useCallback(async () => {
    if (selectedCount === 0) return;
    setDeleteBusy(true);
    try {
      await deleteGenerations(selectedArray);
      setItems((prev) => prev.filter((i) => !selectedIds.has(i.id)));
      setTotal((prev) => prev - selectedCount);
      setSelectedIds(new Set());
      setDeleteOpen(false);
      showToast(`${selectedCount} geração(ões) excluída(s).`, "success");
    } catch {
      showToast("Falha ao excluir gerações.", "error");
    } finally {
      setDeleteBusy(false);
    }
  }, [selectedArray, selectedIds, selectedCount]);

  /* ── Export handler ── */
  const handleExport = useCallback(async () => {
    if (selectedCount === 0) return;
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

  const getFullImageUrl = useCallback((gen: Generation): string => {
    return gen.url || getGenerationDataUrl(gen.id);
  }, []);

  /* ═══════════════════════════════════════════════════════════════════
     RENDER
     ═══════════════════════════════════════════════════════════════════ */

  /* ── Loading state ── */
  if (loading) {
    return (
      <div className="p-4 md:p-6">
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-3">
          {Array.from({ length: 12 }).map((_, i) => (
            <div
              key={i}
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
        {selectedCount > 0 && (
          <span className="font-mono text-2xs text-brand-400">
            {selectedCount} selecionada{selectedCount > 1 ? "s" : ""}
          </span>
        )}
      </div>

      {/* ── Grid ── */}
      <div className="flex-1 overflow-visible p-4 md:p-6 lg:min-h-0 lg:overflow-y-auto">
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5 2xl:grid-cols-6 gap-3">
          {items.map((gen) => {
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
                    toggleSelect(gen.id);
                  }}
                  aria-label={isSelected ? `Desselecionar geração seed ${gen.seed}` : `Selecionar geração seed ${gen.seed}`}
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

                {/* Botão zoom — hover */}
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    setLightboxItem(gen);
                  }}
                  aria-label="Ampliar geração"
                  className="absolute bottom-2 right-2 z-10 flex size-10 items-center justify-center rounded-lg bg-black/60 border border-white/20 text-zinc-200 opacity-0 group-hover:opacity-100 transition-opacity hover:bg-black/80 backdrop-blur-md cursor-pointer"
                >
                  <IconZoomIn className="size-4" />
                </button>

                {/* Click no card = selecionar/desselecionar */}
                <button
                  type="button"
                  onClick={() => toggleSelect(gen.id)}
                  className="absolute inset-0 z-[5] cursor-pointer"
                  aria-label={isSelected ? "Desselecionar" : "Selecionar"}
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
        <div
          role="region"
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
              onClick={selectedCount === total ? clearSelection : selectAll}
              className="flex items-center space-x-1.5 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1 font-mono text-xs text-zinc-300 transition-colors hover:bg-white/10 hover:text-white cursor-pointer"
            >
              <IconCheck className="size-3.5 text-brand-400" />
              <span className="hidden sm:inline">
                {selectedCount === total ? "Desmarcar todas" : "Selecionar todas"}
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
        </div>
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
              {lightboxItem.params?.steps ? (
                <MetaRow label="Steps" value={String(lightboxItem.params.steps)} mono />
              ) : null}
              {lightboxItem.params?.guidance_scale != null ? (
                <MetaRow label="CFG" value={String(lightboxItem.params.guidance_scale)} mono />
              ) : null}
              {lightboxItem.params?.quantization ? (
                <MetaRow label="Quantização" value={String(lightboxItem.params.quantization)} />
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
