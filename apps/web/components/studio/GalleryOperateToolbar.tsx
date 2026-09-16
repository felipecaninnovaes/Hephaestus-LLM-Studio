"use client";

import React from "react";
import { IconGrid, IconList, IconBoxSelect, IconCheck } from "@/components/icons";
import { SubmodulePills } from "@/components/ui";
import type { StudioClass } from "@/types/studio";

export type GallerySplitView = "all" | "train" | "val" | "test" | "trash";
export type GalleryAnnotationFilter = "all" | "labeled" | "unlabeled";
export type GalleryDensity = "compact" | "normal" | "table";

export interface GalleryOperateToolbarProps {
  currentView: GallerySplitView;
  onViewChange: (view: GallerySplitView) => void;
  annotationFilter: GalleryAnnotationFilter;
  onAnnotationFilterChange: (filter: GalleryAnnotationFilter) => void;
  density: GalleryDensity;
  onDensityChange: (density: GalleryDensity) => void;
  selectionMode: boolean;
  onToggleSelectionMode: () => void;
  selectedCount: number;
  totalActive: number;
  totalTrash: number;
  classes?: StudioClass[];
  selectedClassId?: string | null;
  onClassChange?: (classId: string | null) => void;
  isAllSelected?: boolean;
  onSelectAll?: () => void;
  onClearSelection?: () => void;
}

export function GalleryOperateToolbar({
  currentView,
  onViewChange,
  annotationFilter,
  onAnnotationFilterChange,
  density,
  onDensityChange,
  selectionMode,
  onToggleSelectionMode,
  selectedCount,
  totalActive,
  totalTrash,
  classes,
  selectedClassId,
  onClassChange,
  isAllSelected = false,
  onSelectAll,
  onClearSelection,
}: GalleryOperateToolbarProps) {
  return (
    <div className="flex flex-col gap-3 rounded-2xl border border-zinc-800/80 bg-zinc-950/70 p-3 shadow-lg backdrop-blur-xl">
      <div className="flex flex-wrap items-center justify-between gap-2.5">
        {/* Pílulas de Submódulo / Splits */}
        <div className="flex items-center space-x-1 overflow-x-auto min-w-0">
          <SubmodulePills<GallerySplitView>
            value={currentView}
            onChange={onViewChange}
            items={[
              { id: "all", label: "Todas", count: totalActive },
              { id: "train", label: "Treino" },
              { id: "val", label: "Validação" },
              { id: "test", label: "Teste" },
              ...(totalTrash > 0
                ? [{ id: "trash" as GallerySplitView, label: "Lixeira", count: totalTrash }]
                : []),
            ]}
          />
        </div>

        {/* Controles de Densidade e Modo Seleção */}
        <div className="flex items-center space-x-2 shrink-0">
          {/* Filtro de Rotação/Anotação (só se não for lixeira) */}
          {currentView !== "trash" && (
            <div className="hidden sm:flex items-center rounded-lg border border-white/10 bg-zinc-900/90 p-0.5 font-mono text-2xs">
              <button
                type="button"
                onClick={() => onAnnotationFilterChange("all")}
                className={`rounded px-2 py-1 transition-colors cursor-pointer ${
                  annotationFilter === "all"
                    ? "bg-brand-500/20 text-brand-300 font-semibold"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                Todas
              </button>
              <button
                type="button"
                onClick={() => onAnnotationFilterChange("labeled")}
                className={`rounded px-2 py-1 transition-colors cursor-pointer ${
                  annotationFilter === "labeled"
                    ? "bg-status-success/20 text-[#a7f3d0] font-semibold"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                Rotuladas
              </button>
              <button
                type="button"
                onClick={() => onAnnotationFilterChange("unlabeled")}
                className={`rounded px-2 py-1 transition-colors cursor-pointer ${
                  annotationFilter === "unlabeled"
                    ? "bg-status-alert/20 text-amber-300 font-semibold"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                Sem rótulo
              </button>
            </div>
          )}

          {/* Alternador de Densidade */}
          <div className="flex items-center rounded-lg border border-white/10 bg-zinc-900/90 p-0.5">
            <button
              type="button"
              onClick={() => onDensityChange("compact")}
              title="Grade compacta (alta densidade)"
              className={`rounded p-1 transition-colors cursor-pointer ${
                density === "compact"
                  ? "bg-brand-500/20 text-brand-300"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <svg aria-hidden="true" focusable="false" className="size-4" viewBox="0 0 16 16" fill="currentColor">
                <rect x="1" y="1" width="3.5" height="3.5" rx="0.5" />
                <rect x="6" y="1" width="3.5" height="3.5" rx="0.5" />
                <rect x="11" y="1" width="3.5" height="3.5" rx="0.5" />
                <rect x="1" y="6" width="3.5" height="3.5" rx="0.5" />
                <rect x="6" y="6" width="3.5" height="3.5" rx="0.5" />
                <rect x="11" y="6" width="3.5" height="3.5" rx="0.5" />
                <rect x="1" y="11" width="3.5" height="3.5" rx="0.5" />
                <rect x="6" y="11" width="3.5" height="3.5" rx="0.5" />
                <rect x="11" y="11" width="3.5" height="3.5" rx="0.5" />
              </svg>
            </button>
            <button
              type="button"
              onClick={() => onDensityChange("normal")}
              title="Grade padrão"
              className={`rounded p-1 transition-colors cursor-pointer ${
                density === "normal"
                  ? "bg-brand-500/20 text-brand-300"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <IconGrid className="size-4" />
            </button>
            <button
              type="button"
              onClick={() => onDensityChange("table")}
              title="Modo tabela detalhada"
              className={`rounded p-1 transition-colors cursor-pointer ${
                density === "table"
                  ? "bg-brand-500/20 text-brand-300"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <IconList className="size-4" />
            </button>
          </div>

          {/* Botão Selecionar Todas */}
          {onSelectAll && (
            <button
              type="button"
              onClick={isAllSelected ? onClearSelection : onSelectAll}
              title={isAllSelected ? "Desmarcar todas" : "Selecionar todas as amostras"}
              className={`flex items-center space-x-1.5 rounded-lg border px-2.5 py-1.5 font-mono text-xs transition-colors cursor-pointer ${
                isAllSelected
                  ? "border-brand-500/40 bg-brand-500/20 text-brand-300 font-semibold shadow-sm"
                  : "border-white/10 bg-zinc-900/90 text-zinc-300 hover:bg-white/5 hover:text-white"
              }`}
            >
              <IconCheck className="size-3.5 text-brand-400" />
              <span>{isAllSelected ? "Desmarcar todas" : "Selecionar todas"}</span>
            </button>
          )}

          {/* Botão de Modo Seleção */}
          <button
            type="button"
            onClick={onToggleSelectionMode}
            title={selectionMode ? "Sair do modo seleção" : "Entrar no modo seleção"}
            className={`flex items-center space-x-1.5 rounded-lg border px-2.5 py-1.5 font-mono text-xs transition-colors cursor-pointer ${
              selectionMode
                ? "border-brand-500/40 bg-brand-500/20 text-brand-300 font-semibold"
                : "border-white/10 bg-zinc-900/90 text-zinc-300 hover:bg-white/5 hover:text-white"
            }`}
          >
            <IconBoxSelect className="size-3.5" />
            <span className="hidden sm:inline">
              {selectionMode ? `Seleção (${selectedCount})` : "Selecionar"}
            </span>
          </button>
        </div>
      </div>

      {/* Seletor de Classe / Filtro Estrito */}
      {currentView !== "trash" && classes && classes.length > 0 && (
        <div className="flex items-center space-x-1.5 overflow-x-auto min-w-0 pt-2 border-t border-white/5">
          <span className="font-mono text-3xs text-zinc-500 uppercase tracking-wider shrink-0 mr-1">
            Classe:
          </span>
          <button
            type="button"
            onClick={() => onClassChange?.(null)}
            className={`rounded-lg px-2.5 py-1 font-mono text-2xs transition-colors shrink-0 cursor-pointer ${
              !selectedClassId
                ? "bg-brand-500/20 text-brand-300 font-semibold border border-brand-500/30"
                : "bg-white/[0.03] text-zinc-400 hover:text-zinc-200 border border-white/5"
            }`}
          >
            Todas
          </button>
          {classes.map((c) => (
            <button
              key={c.id}
              type="button"
              onClick={() => onClassChange?.(selectedClassId === c.id ? null : c.id)}
              className={`rounded-lg px-2.5 py-1 font-mono text-2xs transition-colors shrink-0 cursor-pointer ${
                selectedClassId === c.id
                  ? "bg-status-alert/25 text-amber-200 font-semibold border border-status-alert/40 shadow-sm"
                  : "bg-white/[0.03] text-zinc-400 hover:text-zinc-200 border border-white/5"
              }`}
            >
              {c.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export default GalleryOperateToolbar;
