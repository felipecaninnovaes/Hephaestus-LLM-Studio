"use client";

import React from "react";
import { IconTrash, IconX, IconCheck, IconSparkles } from "@/components/icons";

export interface FloatingSelectionBarProps {
  selectedCount: number;
  totalInView: number;
  onSelectAll: () => void;
  onClearSelection: () => void;
  onBatchDelete: () => void;
  onAutoLabel?: () => void;
  busy?: boolean;
}

export function FloatingSelectionBar({
  selectedCount,
  totalInView,
  onSelectAll,
  onClearSelection,
  onBatchDelete,
  onAutoLabel,
  busy = false,
}: FloatingSelectionBarProps) {
  if (selectedCount === 0) return null;

  const isAllSelected = selectedCount === totalInView && totalInView > 0;

  return (
    <div
      role="region"
      aria-label="Ações para imagens selecionadas"
      className="fixed bottom-6 left-1/2 -translate-x-1/2 z-40 flex items-center space-x-3 rounded-2xl border border-brand-500/30 bg-zinc-950/95 px-4 py-2.5 shadow-2xl backdrop-blur-xl animate-in fade-in slide-in-from-bottom-5"
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
        <span className="font-mono text-xs text-zinc-300">
          {selectedCount === 1 ? "selecionada" : "selecionadas"}
        </span>
      </div>

      <div className="flex items-center space-x-2">
        <button
          type="button"
          onClick={isAllSelected ? onClearSelection : onSelectAll}
          className="flex items-center space-x-1.5 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1 font-mono text-xs text-zinc-300 transition-colors hover:bg-white/10 hover:text-white cursor-pointer"
        >
          <IconCheck className="size-3.5 text-brand-400" />
          <span>{isAllSelected ? "Desmarcar tudo" : "Marcar todas"}</span>
        </button>

        {onAutoLabel && (
          <button
            type="button"
            onClick={onAutoLabel}
            disabled={busy}
            className="flex items-center space-x-1.5 rounded-lg border border-brand-500/40 bg-brand-500/15 px-3 py-1 font-mono text-xs font-semibold text-brand-300 transition-colors hover:bg-brand-500/25 disabled:opacity-60 cursor-pointer shadow-sm"
          >
            <IconSparkles className="size-3.5 text-brand-400" />
            <span>AutoLabel ({selectedCount})</span>
          </button>
        )}

        <button
          type="button"
          onClick={onBatchDelete}
          disabled={busy}
          className="flex items-center space-x-1.5 rounded-lg border border-rose-500/40 bg-rose-500/15 px-3 py-1 font-mono text-xs font-semibold text-rose-300 transition-colors hover:bg-rose-500/25 disabled:opacity-60 cursor-pointer"
        >
          <IconTrash className="size-3.5 text-rose-400" />
          <span>{busy ? "Movendo…" : "Mover para Lixeira"}</span>
        </button>

        <button
          type="button"
          onClick={onClearSelection}
          aria-label="Cancelar seleção"
          title="Cancelar seleção"
          className="rounded-lg p-1 text-zinc-400 hover:bg-white/5 hover:text-white transition-colors cursor-pointer"
        >
          <IconX className="size-4" />
        </button>
      </div>
    </div>
  );
}

export default FloatingSelectionBar;
