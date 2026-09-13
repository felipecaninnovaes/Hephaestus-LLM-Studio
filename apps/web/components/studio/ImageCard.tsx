"use client";

import React from "react";
import {
  IconSearch,
  IconTrash,
  IconCheck,
  IconZoomIn,
  IconBoxSelect,
  IconSparkles,
} from "@/components/icons";
import type { ImageItem } from "@/types/studio";

export interface ImageCardProps {
  item: ImageItem;
  variant?: "active" | "search" | "trash";
  onClick?: () => void;
  onDelete?: () => void;
  onSearchSimilar?: () => void;
  onRestore?: () => void;
  isRestoring?: boolean;
  searchScore?: number;
  actionText?: string;
  className?: string;
  selected?: boolean;
  selectionMode?: boolean;
  onSelect?: (selected: boolean) => void;
  onQuickLook?: () => void;
  density?: "compact" | "normal";
}

export function ImageCard({
  item,
  variant = "active",
  onClick,
  onDelete,
  onSearchSimilar,
  onRestore,
  isRestoring = false,
  searchScore,
  actionText,
  className = "",
  selected = false,
  selectionMode = false,
  onSelect,
  onQuickLook,
  density = "normal",
}: ImageCardProps) {
  const isClickable = Boolean(onClick) && variant !== "trash";
  const heightClass = density === "compact" ? "h-24 sm:h-28" : "h-28 sm:h-36";

  return (
    <div
      onClick={onClick}
      role={isClickable ? "button" : undefined}
      tabIndex={isClickable ? 0 : undefined}
      onKeyDown={(e) => {
        if (e.code === "Space" && onQuickLook) {
          e.preventDefault();
          onQuickLook();
        } else if (isClickable && (e.key === "Enter")) {
          e.preventDefault();
          onClick?.();
        }
      }}
      className={`group relative ${heightClass} overflow-hidden rounded-xl border bg-zinc-900/90 transition-all ${
        selected
          ? "border-brand-500 ring-2 ring-brand-500/50 bg-brand-500/10"
          : "border-zinc-800 hover:border-brand-500/60 focus-within:border-brand-500/60"
      } ${isClickable ? "cursor-pointer" : ""} ${className}`}
    >
      <img
        src={item.url}
        alt={item.filename}
        loading="lazy"
        decoding="async"
        className="absolute inset-0 h-full w-full object-cover"
      />
      <div
        className="absolute inset-0 bg-[radial-gradient(#ffffff_1px,transparent_1px)] opacity-20 [background-size:16px_16px]"
        aria-hidden="true"
      />

      {/* Checkbox de Seleção em Massa (Superior Esquerdo) */}
      {variant === "active" && onSelect && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onSelect(!selected);
          }}
          aria-label={selected ? "Desmarcar amostra" : "Selecionar amostra"}
          className={`absolute top-2 left-2 z-20 flex size-5 items-center justify-center rounded border transition-all cursor-pointer ${
            selected
              ? "border-brand-500 bg-brand-500 text-white opacity-100"
              : selectionMode
              ? "border-white/40 bg-black/60 opacity-100 hover:border-white"
              : "border-white/40 bg-black/60 opacity-0 group-hover:opacity-100 hover:border-white"
          }`}
        >
          {selected && <IconCheck className="size-3 stroke-[2.5]" />}
        </button>
      )}

      {/* Botões de Ação Topo (QuickLook / Deletar / Similar) */}
      <div className={`absolute top-2 ${onSelect ? "left-9" : "left-2"} z-10 flex items-center gap-1`}>
        {onQuickLook && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onQuickLook();
            }}
            aria-label="Inspeção rápida (Espaço)"
            title="Inspeção rápida (Espaço)"
            className="rounded-md border border-white/20 bg-zinc-950/90 p-1 text-zinc-300 opacity-0 backdrop-blur-sm transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-white/10 hover:text-white cursor-pointer"
          >
            <IconZoomIn className="size-3.5" />
          </button>
        )}
        {variant === "active" && onDelete && !selectionMode && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            aria-label={`Mover ${item.filename} para a lixeira`}
            title="Mover para a lixeira"
            className="rounded-md border border-rose-500/40 bg-zinc-950/90 p-1 text-rose-300 opacity-0 backdrop-blur-sm transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-rose-500/20 cursor-pointer"
          >
            <IconTrash className="size-3.5" />
          </button>
        )}
        {variant === "active" && onSearchSimilar && !selectionMode && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onSearchSimilar();
            }}
            aria-label={`Buscar similares de ${item.filename}`}
            title="Buscar similares"
            className="rounded-md border border-brand-500/40 bg-zinc-950/90 p-1 text-brand-300 opacity-0 backdrop-blur-sm transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-brand-500/20 cursor-pointer"
          >
            <IconSearch className="size-3.5" />
          </button>
        )}
      </div>

      {/* Badges Superior Direito: Labels + Split ou Score */}
      <div className="absolute top-2 right-2 z-10 flex items-center gap-1">
        {item.boxesCount != null && item.boxesCount > 0 && (
          <span
            title={`${item.boxesCount} ${item.boxesCount === 1 ? "box anotada" : "boxes anotadas"}`}
            className="flex items-center gap-1 rounded border border-[#34d399]/40 bg-black/85 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-[#a7f3d0] backdrop-blur-sm"
          >
            <IconBoxSelect className="size-3 text-[#34d399]" />
            <span>{item.boxesCount}</span>
          </span>
        )}

        {item.caption && (
          <span
            title={`Legenda: "${item.caption}"`}
            className="flex items-center gap-1 rounded border border-brand-500/40 bg-black/85 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-brand-300 backdrop-blur-sm"
          >
            <IconSparkles className="size-3 text-brand-400" />
            <span className="hidden sm:inline">legenda</span>
          </span>
        )}

        {variant === "search" && searchScore != null ? (
          <span
            title="Similaridade (cosseno, -1..1)"
            className="rounded border border-brand-500/30 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-brand-300 backdrop-blur-sm"
          >
            {searchScore.toFixed(2)}
          </span>
        ) : (
          <span className="rounded border border-white/15 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-caps font-semibold text-zinc-300 backdrop-blur-sm">
            {item.split}
          </span>
        )}
      </div>

      {/* Restaurar na Lixeira */}
      {variant === "trash" && onRestore && (
        <button
          type="button"
          onClick={onRestore}
          disabled={isRestoring}
          aria-label={`Restaurar ${item.filename}`}
          className="absolute top-2 left-2 z-10 rounded-lg border border-[#34d399]/40 bg-zinc-950/90 backdrop-blur-sm px-2 py-1 font-mono text-[11px] font-medium text-[#a7f3d0] transition-colors hover:bg-[#34d399]/20 disabled:opacity-60 cursor-pointer"
        >
          {isRestoring ? "Restaurando…" : "Restaurar"}
        </button>
      )}

      {/* Barra Inferior com Nome do Arquivo, Preview de Legenda e Ação */}
      <div className="absolute inset-x-0 bottom-0 flex flex-col border-t border-zinc-800/80 bg-zinc-950/95 px-2.5 py-1.5 font-mono text-[11px] text-zinc-300 backdrop-blur-sm">
        <div className="flex items-center justify-between gap-2">
          <span title={item.filename} className="min-w-0 flex-1 truncate font-semibold">
            {item.filename}
          </span>
          {actionText && (
            <span
              title={actionText}
              className="shrink-0 truncate transition-colors group-hover:text-brand-400 text-[10px]"
            >
              {actionText}
            </span>
          )}
        </div>
        {item.caption && density !== "compact" && (
          <span
            title={item.caption}
            className="truncate text-[10px] text-zinc-400 italic mt-0.5 line-clamp-1"
          >
            &ldquo;{item.caption}&rdquo;
          </span>
        )}
      </div>
    </div>
  );
}

export default ImageCard;
