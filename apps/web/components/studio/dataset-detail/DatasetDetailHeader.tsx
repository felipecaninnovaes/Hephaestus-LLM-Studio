"use client";

import { useRouter } from "next/navigation";
import { useState } from "react";
import {
  IconDatabase,
  IconDownload,
  IconFolder,
  IconLayers,
  IconPlay,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import {
  autoTrackDisabledReason,
  canAutoTrack,
  canTrainYolo,
  trainDisabledReason,
} from "@/lib/datasets";
import { formatBytes } from "@/lib/format";
import type { Dataset } from "@/types/studio";

export interface DatasetDetailHeaderProps {
  dataset: Dataset;
  trashTotal: number;
  exporting: boolean;
  onExport: () => void;
  onOpenClasses: () => void;
  onOpenAutoLabel: () => void;
  onOpenAutoTracker: () => void;
  onOpenImport: () => void;
  onOpenTrain: () => void;
}

export function DatasetDetailHeader({
  dataset,
  trashTotal,
  exporting,
  onExport,
  onOpenClasses,
  onOpenAutoLabel,
  onOpenAutoTracker,
  onOpenImport,
  onOpenTrain,
}: DatasetDetailHeaderProps) {
  const router = useRouter();
  const [actionsOpen, setActionsOpen] = useState(false);

  return (
    <div className="space-y-4">
      <div className="flex flex-col justify-between gap-4 md:flex-row md:items-center">
        <div className="flex items-center space-x-3">
          <Button
            type="button"
            variant="secondary"
            size="icon"
            onClick={() => router.push("/datasets")}
            title="Voltar para a lista de datasets"
            aria-label="Voltar para a lista de datasets"
          >
            <svg
              aria-hidden="true"
              focusable="false"
              className="size-4"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.7"
              viewBox="0 0 24 24"
            >
              <polyline points="15 18 9 12 15 6" />
            </svg>
          </Button>
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm text-brand-400">
            <IconDatabase className="h-5 w-5" />
          </div>
          <div className="min-w-0 flex-1">
            <h2
              title={dataset.title}
              className="truncate text-base font-bold tracking-tight text-white"
            >
              {dataset.title}
            </h2>
            <p className="font-mono text-xs text-zinc-400">
              {dataset.type} · {dataset.imagesCount.toLocaleString()} imagens ·{" "}
              {formatBytes(dataset.sizeBytes)} · {dataset.source ?? "—"}
            </p>
          </div>
        </div>

        {/* Ações Desktop */}
        <div className="hidden flex-wrap items-center gap-2 md:flex">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onOpenClasses}
            title="Renomear, reordenar, criar ou remover classes"
          >
            <IconLayers className="h-4 w-4" />
            <span>Classes</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={dataset.imagesCount === 0}
            title={
              dataset.imagesCount === 0
                ? "Dataset não contém imagens."
                : "Gerar legendas em lote com AutoLabel"
            }
            onClick={() => dataset.imagesCount > 0 && onOpenAutoLabel()}
          >
            <IconSparkles className="h-4 w-4" />
            <span>AutoLabel</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canAutoTrack(dataset)}
            title={autoTrackDisabledReason(dataset)}
            onClick={() => canAutoTrack(dataset) && onOpenAutoTracker()}
          >
            <IconTarget className="h-4 w-4" />
            <span>AutoTracker</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onOpenImport}
            title="Importar backup estruturado (.zip)"
          >
            <IconFolder className="h-4 w-4" />
            <span>Importar</span>
          </Button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={onExport}
            disabled={exporting}
            loading={exporting}
            title="Baixar backup estruturado (.zip)"
          >
            <IconDownload className="h-4 w-4" />
            <span>{exporting ? "Exportando…" : "Exportar"}</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!canTrainYolo(dataset)}
            title={trainDisabledReason(dataset)}
            onClick={() => canTrainYolo(dataset) && onOpenTrain()}
          >
            <IconPlay className="h-4 w-4" />
            <span>Treinar este Dataset</span>
          </Button>
        </div>

        {/* Menu Mobile */}
        <div className="relative md:hidden">
          <Button
            type="button"
            variant="secondary"
            size="icon"
            onClick={() => setActionsOpen((v) => !v)}
            aria-expanded={actionsOpen}
            aria-label="Ações do dataset"
            title="Ações do dataset"
          >
            <span aria-hidden="true" className="text-lg leading-none">
              ⋯
            </span>
          </Button>
          {actionsOpen && (
            <div className="glass-menu absolute right-0 z-30 mt-2 flex w-52 flex-col gap-1 rounded-2xl p-2">
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  onOpenClasses();
                }}
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 cursor-pointer"
              >
                <IconLayers className="h-4 w-4" />
                <span>Classes</span>
              </button>
              <button
                type="button"
                disabled={dataset.imagesCount === 0}
                onClick={() => {
                  if (dataset.imagesCount === 0) return;
                  setActionsOpen(false);
                  onOpenAutoLabel();
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  dataset.imagesCount > 0
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 cursor-pointer"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
              >
                <IconSparkles className="h-4 w-4" />
                <span>AutoLabel</span>
              </button>
              <button
                type="button"
                disabled={!canAutoTrack(dataset)}
                onClick={() => {
                  if (!canAutoTrack(dataset)) return;
                  setActionsOpen(false);
                  onOpenAutoTracker();
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  canAutoTrack(dataset)
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 cursor-pointer"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
              >
                <IconTarget className="h-4 w-4" />
                <span>AutoTracker</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  onOpenImport();
                }}
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 cursor-pointer"
              >
                <IconFolder className="h-4 w-4" />
                <span>Importar</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  setActionsOpen(false);
                  onExport();
                }}
                disabled={exporting}
                className="flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 disabled:opacity-55 cursor-pointer"
              >
                <IconDownload className="h-4 w-4" />
                <span>{exporting ? "Exportando…" : "Exportar"}</span>
              </button>
              <button
                type="button"
                disabled={!canTrainYolo(dataset)}
                onClick={() => {
                  if (!canTrainYolo(dataset)) return;
                  setActionsOpen(false);
                  onOpenTrain();
                }}
                className={`flex h-9 items-center space-x-2 rounded-lg px-3 text-xs font-medium ${
                  canTrainYolo(dataset)
                    ? "text-zinc-200 transition-colors hover:bg-brand-500/[0.12] hover:text-brand-300 cursor-pointer"
                    : "cursor-not-allowed text-zinc-200 opacity-60"
                }`}
              >
                <IconPlay className="h-4 w-4" />
                <span>Treinar este Dataset</span>
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Barra de Estatísticas */}
      <div className="glass-card flex flex-wrap items-center gap-2 rounded-2xl shadow-lg px-4 py-2.5 font-mono text-2xs text-zinc-400">
        <span className="font-semibold text-zinc-200">
          {dataset.imagesCount.toLocaleString()} amostras
        </span>
        <span className="h-3 w-px bg-white/10" />
        <span>
          <span className="text-status-success font-semibold">
            {dataset.labeledCount.toLocaleString()}
          </span>{" "}
          rotuladas
        </span>
        <span className="h-3 w-px bg-white/10" />
        <span>
          <span className="text-zinc-400 font-semibold">
            {(dataset.imagesCount - dataset.labeledCount).toLocaleString()}
          </span>{" "}
          pendentes
        </span>
        <span className="h-3 w-px bg-white/10" />
        <span>{dataset.classes.length} classes</span>
        {trashTotal > 0 && (
          <>
            <span className="h-3 w-px bg-white/10" />
            <span className="text-rose-400">{trashTotal} na lixeira</span>
          </>
        )}
      </div>
    </div>
  );
}
