"use client";

import type { RefObject } from "react";
import { useRouter } from "next/navigation";
import {
  IconFolder,
  IconSearch,
  IconTrash,
} from "@/components/icons";
import type {
  GalleryAnnotationFilter,
  GalleryDensity,
  GallerySplitView,
} from "@/components/studio/GalleryOperateToolbar";
import ImageCard from "@/components/studio/ImageCard";
import ImageTableView from "@/components/studio/ImageTableView";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { Spinner } from "@/components/ui/Spinner";
import type { Dataset, ImageItem, SearchItem } from "@/types/studio";

export interface DatasetImageGridProps {
  dataset: Dataset;
  items: ImageItem[];
  total: number;
  splitView: GallerySplitView;
  density: GalleryDensity;
  annotationFilter: GalleryAnnotationFilter;
  selectionMode: boolean;
  selectedIds: Set<string>;
  onSelectToggle: (id: string, selected: boolean) => void;
  onSelectAll: () => void;
  onClearSelection: () => void;
  onRestore: (item: ImageItem) => void;
  restoringId: string | null;
  onDelete: (item: ImageItem) => void;
  onQuickLook: (item: ImageItem) => void;
  onTileClick: (item: ImageItem) => void;
  results: SearchItem[];
  similarFor: string | null;
  searchMode: "tag" | "semantic";
  activeQuery: string | null;
  activeTag: string | null;
  selectedClassId: string | null;
  onClearSearch: () => void;
  onClearAllFilters: () => void;
  uploading: boolean;
  uploadSent: number;
  uploadCount: number;
  uploadBatchInfo: { batchIndex: number; batchCount: number } | null;
  onUploadClick: () => void;
  onCancelUpload: () => void;
  sentinelRef: RefObject<HTMLDivElement | null>;
  loadingMore: boolean;
}

export function DatasetImageGrid({
  dataset,
  items,
  total,
  splitView,
  density,
  annotationFilter,
  selectionMode,
  selectedIds,
  onSelectToggle,
  onSelectAll,
  onClearSelection,
  onRestore,
  restoringId,
  onDelete,
  onQuickLook,
  onTileClick,
  results,
  similarFor,
  searchMode,
  activeQuery,
  activeTag,
  selectedClassId,
  onClearSearch,
  onClearAllFilters,
  uploading,
  uploadSent,
  uploadCount,
  uploadBatchInfo,
  onUploadClick,
  onCancelUpload,
  sentinelRef,
  loadingMore,
}: DatasetImageGridProps) {
  const router = useRouter();

  if (splitView === "trash") {
    if (total === 0) {
      return (
        <EmptyState
          icon={<IconTrash className="h-5 w-5" />}
          title="A lixeira está vazia"
          description="Imagens movidas para a lixeira aparecerão aqui antes da exclusão definitiva."
          className="py-14 border border-zinc-800"
        />
      );
    }
    if (density === "table") {
      return (
        <ImageTableView
          items={items}
          selectedIds={selectedIds}
          onToggleSelect={onSelectToggle}
          onSelectAll={onSelectAll}
          onClearSelection={onClearSelection}
          onQuickLook={(idx) => {
            const item = items[idx];
            if (item) onQuickLook(item);
          }}
          category={dataset.category}
        />
      );
    }
    return (
      <div
        className={
          density === "compact"
            ? "grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-6 xl:grid-cols-8"
            : "grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4"
        }
      >
        {items.map((item) => (
          <ImageCard
            key={item.id}
            item={item}
            variant="trash"
            density={density}
            selected={selectedIds.has(item.id)}
            selectionMode={selectionMode}
            onSelect={(sel) => onSelectToggle(item.id, sel)}
            onRestore={() => onRestore(item)}
            isRestoring={restoringId === item.id}
            onQuickLook={() => onQuickLook(item)}
          />
        ))}
      </div>
    );
  }

  if (dataset.imagesCount === 0) {
    return (
      <EmptyState
        icon={<IconFolder className="h-5 w-5" />}
        title="Galeria vazia"
        description="Este dataset ainda não tem amostras. Arraste uma pasta ou envie imagens pelo botão abaixo."
        className="py-14 border border-zinc-800"
      >
        <Button
          type="button"
          variant="secondary"
          size="md"
          onClick={onUploadClick}
          disabled={uploading}
          loading={uploading && !uploadSent}
          className="mt-2"
        >
          {uploading
            ? `Enviando ${uploadSent} de ${uploadCount}…`
            : "Enviar amostras"}
        </Button>
        {uploading && uploadBatchInfo && (
          <p className="mt-1.5 font-mono text-2xs text-zinc-400">
            lote {uploadBatchInfo.batchIndex} de {uploadBatchInfo.batchCount}
          </p>
        )}
        {uploading && (
          <Button
            type="button"
            variant="destructive"
            size="sm"
            onClick={onCancelUpload}
            className="mt-2"
          >
            Cancelar envio
          </Button>
        )}
      </EmptyState>
    );
  }

  if (
    similarFor !== null ||
    (searchMode === "semantic" && activeQuery !== null)
  ) {
    return (
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-2xl border border-zinc-800/80 bg-zinc-950/60 backdrop-blur-sm px-4 py-2.5 text-2xs text-zinc-400">
          <span>
            Resultados da busca semântica —{" "}
            <span className="font-mono text-zinc-200">
              {results.length.toLocaleString()}
            </span>{" "}
            {results.length === 1 ? "imagem" : "imagens"}
          </span>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onClearSearch}
          >
            Limpar busca
          </Button>
        </div>
        {results.length === 0 ? (
          <EmptyState
            icon={<IconSearch className="h-5 w-5" />}
            title={
              activeQuery !== null
                ? `Nenhum resultado para '${activeQuery}'`
                : "Nenhuma imagem similar"
            }
            description="Tente ajustar os termos de busca ou utilize uma imagem diferente como referência."
            className="py-14 border border-zinc-800"
          />
        ) : (
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
            {results.map((result) => (
              <ImageCard
                key={result.image.id}
                item={result.image}
                variant="search"
                searchScore={result.score}
                density={density === "compact" ? "compact" : "normal"}
                onClick={() =>
                  router.push(
                    `/datasets/${dataset.id}/annotate/${result.image.id}`,
                  )
                }
                onQuickLook={() => onQuickLook(result.image)}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  if (total === 0) {
    return (
      <EmptyState
        icon={
          activeTag || selectedClassId ? (
            <IconSearch className="h-5 w-5" />
          ) : (
            <IconFolder className="h-5 w-5" />
          )
        }
        title={
          activeTag || selectedClassId
            ? "Nenhuma imagem encontrada com o filtro aplicado"
            : annotationFilter !== "all"
              ? "Nenhuma imagem encontrada com o filtro de rótulo"
              : "Nenhuma imagem ativa neste conjunto"
        }
        description={
          activeTag || selectedClassId
            ? "Tente buscar por outro termo ou selecione outra classe."
            : "Ajuste os filtros da barra de ferramentas ou adicione mais amostras."
        }
        className="py-14 border border-zinc-800"
      >
        {(activeTag || selectedClassId) && (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onClearAllFilters}
            className="mt-2"
          >
            Limpar filtros
          </Button>
        )}
      </EmptyState>
    );
  }

  if (density === "table") {
    return (
      <div className="flex flex-col gap-4">
        <ImageTableView
          items={items}
          selectedIds={selectedIds}
          onToggleSelect={onSelectToggle}
          onSelectAll={onSelectAll}
          onClearSelection={onClearSelection}
          onOpenAnnotate={onTileClick}
          onDelete={onDelete}
          onQuickLook={(idx) => {
            const item = items[idx];
            if (item) onQuickLook(item);
          }}
          category={dataset.category}
        />
        <div ref={sentinelRef} className="h-10 flex items-center justify-center">
          {loadingMore && <Spinner className="size-5 text-zinc-500" />}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div
        className={
          density === "compact"
            ? "grid grid-cols-3 gap-2 sm:grid-cols-4 md:grid-cols-6 xl:grid-cols-8"
            : "grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4"
        }
      >
        {items.map((item) => (
          <ImageCard
            key={item.id}
            item={item}
            density={density}
            selected={selectedIds.has(item.id)}
            selectionMode={selectionMode}
            onSelect={(sel) => onSelectToggle(item.id, sel)}
            onClick={() => onTileClick(item)}
            onDelete={() => onDelete(item)}
            onQuickLook={() => onQuickLook(item)}
          />
        ))}
      </div>

      <div ref={sentinelRef} className="h-10 flex items-center justify-center">
        {loadingMore && <Spinner className="size-5 text-zinc-500" />}
      </div>
    </div>
  );
}
