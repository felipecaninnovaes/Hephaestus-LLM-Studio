"use client";

import { useState } from "react";
import {
  IconCheck,
  IconPlus,
  IconTarget,
  IconTrash,
  IconZoomIn,
} from "@/components/icons";
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
  onUploadClick?: () => void;
  uploading?: boolean;
  uploadSent?: number;
  uploadCount?: number;
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
  onUploadClick,
  uploading = false,
  uploadSent = 0,
  uploadCount = 0,
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
            <th className="py-2.5 px-3 w-40 text-right pr-4">
              {onUploadClick ? (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onUploadClick();
                  }}
                  disabled={uploading}
                  className="inline-flex min-h-[32px] items-center gap-1.5 rounded-lg border border-brand-500/40 bg-brand-500/15 px-2.5 py-1 text-2xs font-mono font-medium text-brand-300 transition-colors hover:border-brand-500 hover:bg-brand-500/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-50 disabled:pointer-events-none cursor-pointer"
                  title="Adicionar novas imagens ao dataset"
                  aria-label="Adicionar imagens"
                >
                  {uploading ? (
                    <span>Enviando {uploadSent} de {uploadCount}…</span>
                  ) : (
                    <>
                      <IconPlus className="size-3.5" />
                      <span>Adicionar</span>
                    </>
                  )}
                </button>
              ) : (
                "Ações"
              )}
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-zinc-800/50">
          {items.map((item, idx) => (
            <ImageTableRow
              key={item.id}
              item={item}
              idx={idx}
              isSelected={selectedIds.has(item.id)}
              onToggleSelect={onToggleSelect}
              onQuickLook={onQuickLook}
              onOpenAnnotate={onOpenAnnotate}
              onDelete={onDelete}
              category={category}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface ImageTableRowProps {
  item: ImageItem;
  idx: number;
  isSelected: boolean;
  onToggleSelect: (id: string, selected: boolean) => void;
  onQuickLook: (index: number) => void;
  onOpenAnnotate?: (item: ImageItem) => void;
  onDelete?: (item: ImageItem) => void;
  category?: string;
}

function ImageTableRow({
  item,
  idx,
  isSelected,
  onToggleSelect,
  onQuickLook,
  onOpenAnnotate,
  onDelete,
  category,
}: ImageTableRowProps) {
  const [thumbFailed, setThumbFailed] = useState(false);
  const imageSrc = thumbFailed ? item.url : item.thumbUrl;

  return (
    <tr
      onClick={() => onQuickLook(idx)}
      className={`transition-colors hover:bg-white/[0.04] cursor-pointer ${
        isSelected ? "bg-brand-500/10" : ""
      }`}
    >
      <td className="py-2 pl-4 pr-2">
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
          src={imageSrc}
          alt={item.filename}
          loading="lazy"
          decoding="async"
          onError={() => {
            if (!thumbFailed) {
              setThumbFailed(true);
            }
          }}
          className="size-10 rounded-lg object-cover border border-zinc-800 bg-zinc-900"
        />
      </td>

      <td className="py-2 px-3 text-zinc-200">
        <span
          title={item.filename}
          className="truncate max-w-[280px] inline-block font-medium"
        >
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

      <td className="py-2 px-3 text-right pr-4">
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
}

export default ImageTableView;
