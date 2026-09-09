"use client";

import React from "react";
import { IconSearch, IconTrash } from "@/components/icons";
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
}: ImageCardProps) {
  const isClickable = Boolean(onClick) && variant !== "trash";

  return (
    <div
      onClick={onClick}
      role={isClickable ? "button" : undefined}
      tabIndex={isClickable ? 0 : undefined}
      onKeyDown={(e) => {
        if (isClickable && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onClick?.();
        }
      }}
      className={`group relative h-28 sm:h-36 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900/90 transition-all hover:border-brand-500/60 focus-within:border-brand-500/60 ${
        isClickable ? "cursor-pointer" : ""
      } ${className}`}
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

      {/* Badge Superior Direito: Split ou Score de Similaridade */}
      {variant === "search" && searchScore != null ? (
        <span
          title="Similaridade (cosseno, -1..1)"
          className="absolute top-2 right-2 rounded border border-brand-500/30 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[11px] font-semibold text-brand-300 backdrop-blur-sm"
        >
          {searchScore.toFixed(2)}
        </span>
      ) : (
        <span className="absolute top-2 right-2 rounded border border-white/15 bg-zinc-950/90 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-caps font-semibold text-zinc-300 backdrop-blur-sm">
          {item.split}
        </span>
      )}

      {/* Ações Superiores Esquerdas */}
      {variant === "active" && (onDelete || onSearchSimilar) && (
        <div className="absolute top-2 left-2 flex items-center gap-1.5 z-10">
          {onDelete && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onDelete();
              }}
              aria-label={`Mover ${item.filename} para a lixeira`}
              title="Mover para a lixeira"
              className="rounded-lg border border-rose-500/40 bg-zinc-950/90 p-1.5 text-rose-300 opacity-0 backdrop-blur-sm transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-rose-500/20 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-rose-500"
            >
              <IconTrash className="h-3.5 w-3.5" />
            </button>
          )}
          {onSearchSimilar && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onSearchSimilar();
              }}
              aria-label={`Buscar similares de ${item.filename}`}
              title="Buscar similares"
              className="rounded-lg border border-brand-500/40 bg-zinc-950/90 p-1.5 text-brand-300 opacity-0 backdrop-blur-sm transition-all group-hover:opacity-100 focus-visible:opacity-100 hover:bg-brand-500/20 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
            >
              <IconSearch className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      )}

      {variant === "trash" && onRestore && (
        <button
          type="button"
          onClick={onRestore}
          disabled={isRestoring}
          aria-label={`Restaurar ${item.filename}`}
          className="absolute top-2 left-2 z-10 rounded-lg border border-[#34d399]/40 bg-zinc-950/90 backdrop-blur-sm px-2 py-1 font-mono text-[11px] font-medium text-[#a7f3d0] transition-colors hover:bg-[#34d399]/20 disabled:opacity-60 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
        >
          {isRestoring ? "Restaurando…" : "Restaurar"}
        </button>
      )}

      {/* Barra Inferior com Nome do Arquivo e Ação */}
      <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-2 border-t border-zinc-800/80 bg-zinc-950/90 px-2.5 py-1.5 font-mono text-[11px] text-zinc-300 backdrop-blur-sm">
        <span title={item.filename} className="min-w-0 flex-1 truncate">
          {item.filename}
        </span>
        {actionText && (
          <span
            title={actionText}
            className="shrink-0 truncate transition-colors group-hover:text-brand-400"
          >
            {actionText}
          </span>
        )}
      </div>
    </div>
  );
}

export default ImageCard;
