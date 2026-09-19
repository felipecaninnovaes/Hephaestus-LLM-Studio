"use client";

import React from "react";
import { IconTrash, IconZoomIn, IconTarget, IconCheck } from "@/components/icons";
import { formatBytes } from "@/lib/format";
import type { ImageItem } from "@/types/studio";

export interface ImageTableViewProps {
  items: ImageItem[];
  selectedIds: Set<string>;
  onToggleSelect: (id: string, selected: boolean) => void;
  onSelectAll?: () => void;
  onClearSelection?: () => void;
  onQuickLook: (index: number) => void;
  onOpenAnnotate?: (item: ImageItem) => void;
  onDelete?: (item: ImageItem) => void;
  category?: string;
}

export function ImageTableView({
  items,
  selectedIds,
  onToggleSelect,
  onSelectAll,
  onClearSelection,
  onQuickLook,
  onOpenAnnotate,
  onDelete,
  category,
}: ImageTableViewProps) {

  const allSelected = items.length > 0 && items.every((i) => selectedIds.has(i.id));

  return (
    <div className="overflow-x-auto rounded-2xl border border-zinc-800/80 bg-zinc-950/80 shadow-lg backdrop-blur-xl">
      <table className="w-full text-left text-xs font-mono">
        <thead className="border-b border-zinc-800/80 bg-zinc-900/60 text-zinc-400 uppercase tracking-caps text-3xs">
          <tr>
            <th className="py-2.5 pl-4 pr-2 w-8">
              <button
                type="button"
                onClick={allSelected ? onClearSelection : onSelectAll}
                aria-label={allSelected ? "Desmarcar todas" : "Selecionar todas"}
                className={`flex size-4 items-center justify-center rounded border transition-colors cursor-pointer ${
                  allSelected
                    ? "border-brand-500 bg-brand-500 text-white"
                    : "border-white/30 bg-black/40 hover:border-white"
                }`}
              >
                {allSelected && <IconCheck className="size-2.5 stroke-[3]" />}
              </button>
            </th>
            <th className="py-2.5 px-3 w-14">Amostra</th>
            <th className="py-2.5 px-3">Nome Canônico</th>
            <th className="py-2.5 px-3 w-24">Split</th>
            <th className="py-2.5 px-3 w-28">Resolução</th>
            <th className="py-2.5 px-3 w-24">Tamanho</th>
            <th className="py-2.5 px-3 w-24 text-right pr-4">Ações</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/50">
          {items.map((item, idx) => {
            const isSelected = selectedIds.has(item.id);
            return (
              <tr
                key={item.id}
                onClick={() => onQuickLook(idx)}
                className={`transition-colors hover:bg-white/[0.04] cursor-pointer ${
                  isSelected ? "bg-brand-500/10" : ""
                }`}
              >
                <td
                  className="py-2 pl-4 pr-2"
                >
                  <button
                    type="button"
                    aria-label={isSelected ? "Desmarcar" : "Marcar"}
                    onClick={(e) => {
                      e.stopPropagation();
                      onToggleSelect(item.id, !isSelected);
                    }}
                    className={`flex size-4 items-center justify-center rounded border transition-colors cursor-pointer ${
                      isSelected
                        ? "border-brand-500 bg-brand-500 text-white"
                        : "border-white/30 bg-black/40 hover:border-white"
                    }`}
                  >
                    {isSelected && <IconCheck className="size-2.5 stroke-[3]" />}
                  </button>
                </td>

                <td className="py-2 px-3">
                  <img
                    src={item.url}
                    alt={item.filename}
                    loading="lazy"
                    className="size-10 rounded-lg object-cover border border-zinc-800 bg-zinc-900"
                  />
                </td>

                <td className="py-2 px-3 text-zinc-200">
                  <span title={item.filename} className="truncate max-w-[280px] inline-block font-medium">
                    {item.filename}
                  </span>
                </td>

                <td className="py-2 px-3">
                  <span className="rounded border border-white/10 bg-zinc-900 px-1.5 py-0.5 text-3xs uppercase font-semibold text-zinc-300">
                    {item.split}
                  </span>
                </td>

                <td className="py-2 px-3 text-zinc-400">
                  {item.width && item.height ? `${item.width}×${item.height}` : "—"}
                </td>

                <td className="py-2 px-3 text-zinc-400">
                  {formatBytes(item.bytes)}
                </td>

                <td
                  className="py-2 px-3 text-right pr-4"
                >
                  <div className="flex items-center justify-end space-x-1">
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        onQuickLook(idx);
                      }}
                      title="Inspeção rápida (Espaço)"
                      className="rounded p-1 text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors cursor-pointer"
                    >
                      <IconZoomIn className="size-3.5" />
                    </button>

                    {category === "yolo" && (
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          onOpenAnnotate?.(item);
                        }}
                        title="Abrir editor BBox"
                        className="rounded p-1 text-brand-400 hover:text-brand-300 hover:bg-brand-500/10 transition-colors cursor-pointer"
                      >
                        <IconTarget className="size-3.5" />
                      </button>
                    )}

                    {onDelete && (
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          onDelete(item);
                        }}
                        title="Mover para a lixeira"
                        className="rounded p-1 text-rose-400 hover:text-rose-300 hover:bg-rose-500/10 transition-colors cursor-pointer"
                      >
                        <IconTrash className="size-3.5" />
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export default ImageTableView;
