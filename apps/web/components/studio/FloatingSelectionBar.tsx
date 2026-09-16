"use client";

import type React from "react";
import { IconTrash, IconX, IconCheck, IconSparkles, IconTag } from "@/components/icons";

/* ═══════════════════════════════════════════════════════════════════
   FloatingSelectionBar — barra flutuante de ações em lote (G.8 D6)
   Genérico: dataset actions (AutoLabel, EditClasses) + gallery actions
   (Export, Compare) via slots configuráveis via props.

   Decisão: props configuráveis (não componente irmão) — mantém
   compatibilidade total com datasets existentes e evita duplicação
   de estilos. Cada consumidor passa só os callbacks que usa.
   ═══════════════════════════════════════════════════════════════════ */

export interface FloatingSelectionBarAction {
  key: string;
  label: string;
  icon?: React.ReactNode;
  onClick: () => void;
  variant?: "default" | "brand" | "danger";
  disabled?: boolean;
  busy?: boolean;
  hidden?: boolean;
}

export interface FloatingSelectionBarProps {
  selectedCount: number;
  totalInView: number;
  onSelectAll: () => void;
  onClearSelection: () => void;
  onBatchDelete?: () => void;
  /* ── Legacy dataset actions (backward compat) ── */
  onAutoLabel?: () => void;
  onBatchEditClasses?: () => void;
  /* ── Generic extra actions (gallery, etc.) ── */
  extraActions?: FloatingSelectionBarAction[];
  busy?: boolean;
  /* ── Label customizável do botão de exclusão ── */
  deleteLabel?: string;
  /* ── Se true, muestra "bottom-sheet" no mobile ── */
  mobileSheet?: boolean;
}

export function FloatingSelectionBar({
  selectedCount,
  totalInView,
  onSelectAll,
  onClearSelection,
  onBatchDelete,
  onAutoLabel,
  onBatchEditClasses,
  extraActions,
  busy = false,
  deleteLabel,
  mobileSheet = false,
}: FloatingSelectionBarProps) {
  if (selectedCount === 0) return null;

  const isAllSelected = selectedCount === totalInView && totalInView > 0;

  return (
    <section
      aria-label="Ações para itens selecionados"
      className={`
        fixed z-40 flex items-center space-x-3 rounded-2xl border border-brand-500/30 bg-zinc-950/95 px-4 py-2.5 shadow-2xl backdrop-blur-xl animate-in fade-in slide-in-from-bottom-5
        ${mobileSheet
          ? "bottom-0 left-0 right-0 rounded-b-none sm:bottom-6 sm:left-1/2 sm:-translate-x-1/2 sm:rounded-2xl sm:max-w-[calc(100vw-2rem)]"
          : "bottom-6 left-1/2 -translate-x-1/2 max-w-[calc(100vw-2rem)]"
        }
      `}
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
          <span>{isAllSelected ? "Desmarcar todas" : "Selecionar todas"}</span>
        </button>

        {/* Legacy dataset actions */}
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

        {onBatchEditClasses && (
          <button
            type="button"
            onClick={onBatchEditClasses}
            disabled={busy}
            className="flex items-center space-x-1.5 rounded-lg border border-white/15 bg-white/[0.07] px-3 py-1 font-mono text-xs font-semibold text-zinc-200 transition-colors hover:bg-white/15 disabled:opacity-60 cursor-pointer shadow-sm"
          >
            <IconTag className="size-3.5 text-zinc-300" />
            <span>Editar Classes</span>
          </button>
        )}

        {/* Generic extra actions */}
        {extraActions?.filter((a) => !a.hidden).map((action) => (
          <button
            key={action.key}
            type="button"
            onClick={action.onClick}
            disabled={action.disabled || action.busy}
            className={`flex items-center space-x-1.5 rounded-lg px-3 py-1 font-mono text-xs font-semibold transition-colors disabled:opacity-60 cursor-pointer ${
              action.variant === "danger"
                ? "border border-rose-500/40 bg-rose-500/15 text-rose-300 hover:bg-rose-500/25"
                : action.variant === "brand"
                  ? "border border-brand-500/40 bg-brand-500/15 text-brand-300 hover:bg-brand-500/25 shadow-sm"
                  : "border border-white/15 bg-white/[0.07] text-zinc-200 hover:bg-white/15 shadow-sm"
            }`}
          >
            {action.icon}
            <span>{action.busy ? "Aguarde…" : action.label}</span>
          </button>
        ))}

        {/* Delete button (legacy or gallery) */}
        {onBatchDelete && (
          <button
            type="button"
            onClick={onBatchDelete}
            disabled={busy}
            className="flex items-center space-x-1.5 rounded-lg border border-rose-500/40 bg-rose-500/15 px-3 py-1 font-mono text-xs font-semibold text-rose-300 transition-colors hover:bg-rose-500/25 disabled:opacity-60 cursor-pointer"
          >
            <IconTrash className="size-3.5 text-rose-400" />
            <span>{busy ? "Movendo…" : (deleteLabel || "Mover para Lixeira")}</span>
          </button>
        )}

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
    </section>
  );
}

export default FloatingSelectionBar;
