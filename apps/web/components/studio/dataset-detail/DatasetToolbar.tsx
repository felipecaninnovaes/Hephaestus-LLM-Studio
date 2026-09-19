"use client";

import GalleryOperateToolbar, {
  type GalleryAnnotationFilter,
  type GalleryDensity,
  type GallerySplitView,
} from "@/components/studio/GalleryOperateToolbar";
import { Button } from "@/components/ui/Button";
import { SearchInput } from "@/components/ui/SearchInput";
import type { Dataset, SearchStatus } from "@/types/studio";

export interface DatasetToolbarProps {
  dataset: Dataset;
  trashTotal: number;
  splitView: GallerySplitView;
  onSplitChange: (s: GallerySplitView) => void;
  annotationFilter: GalleryAnnotationFilter;
  onAnnotationFilterChange: (a: GalleryAnnotationFilter) => void;
  density: GalleryDensity;
  onDensityChange: (d: GalleryDensity) => void;
  selectionMode: boolean;
  onToggleSelectionMode: () => void;
  selectedCount: number;
  isAllSelected: boolean;
  onSelectAll: () => void;
  onClearSelection: () => void;
  selectedClassId: string | null;
  onClassChange: (clsId: string | null) => void;
  searchMode: "tag" | "semantic";
  setSearchMode: (m: "tag" | "semantic") => void;
  searchInput: string;
  setSearchInput: (val: string) => void;
  onClearSearch: () => void;
  onSearchSubmit: (q: string, m: "tag" | "semantic") => void;
  searching: boolean;
  searchStatus: SearchStatus | null;
  indexBusy: boolean;
  onTriggerIndex: () => void;
  activeTag: string | null;
  total: number;
  onClearAllFilters: () => void;
}

export function DatasetToolbar({
  dataset,
  trashTotal,
  splitView,
  onSplitChange,
  annotationFilter,
  onAnnotationFilterChange,
  density,
  onDensityChange,
  selectionMode,
  onToggleSelectionMode,
  selectedCount,
  isAllSelected,
  onSelectAll,
  onClearSelection,
  selectedClassId,
  onClassChange,
  searchMode,
  setSearchMode,
  searchInput,
  setSearchInput,
  onClearSearch,
  onSearchSubmit,
  searching,
  searchStatus,
  indexBusy,
  onTriggerIndex,
  activeTag,
  total,
  onClearAllFilters,
}: DatasetToolbarProps) {
  return (
    <div className="space-y-3">
      <GalleryOperateToolbar
        currentView={splitView}
        onViewChange={onSplitChange}
        totalActive={dataset.imagesCount}
        totalTrash={trashTotal}
        annotationFilter={annotationFilter}
        onAnnotationFilterChange={onAnnotationFilterChange}
        density={density}
        onDensityChange={onDensityChange}
        selectionMode={selectionMode}
        onToggleSelectionMode={onToggleSelectionMode}
        selectedCount={selectedCount}
        isAllSelected={isAllSelected}
        onSelectAll={onSelectAll}
        onClearSelection={onClearSelection}
        classes={dataset.classes ?? []}
        selectedClassId={selectedClassId}
        onClassChange={onClassChange}
      />

      {splitView !== "trash" && (
        <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-2">
          {/* Alternador de Modo de Filtro / Busca */}
          <div className="inline-flex rounded-xl border border-white/10 bg-zinc-950/80 p-0.5 shrink-0">
            <button
              type="button"
              onClick={() => setSearchMode("tag")}
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
              onClick={() => setSearchMode("semantic")}
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
              onClear={onClearSearch}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  onSearchSubmit(searchInput, searchMode);
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
              onClick={onTriggerIndex}
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
                Classe:{" "}
                {dataset.classes?.find((c) => c.id === selectedClassId)?.name ??
                  selectedClassId}
              </span>
            )}
            <span className="text-zinc-400">
              ({total.toLocaleString()} imagens encontradas)
            </span>
          </div>
          <button
            type="button"
            onClick={onClearAllFilters}
            className="text-2xs text-zinc-400 hover:text-white underline cursor-pointer shrink-0"
          >
            Limpar filtros
          </button>
        </div>
      )}
    </div>
  );
}
