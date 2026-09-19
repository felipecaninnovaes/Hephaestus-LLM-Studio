"use client";

import { useRouter } from "next/navigation";
import {
  IconDownload,
  IconRefresh,
  IconTarget,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { GlassCard } from "@/components/ui/GlassCard";
import { Spinner } from "@/components/ui/Spinner";
import type { Job, PredictionsData } from "@/types/studio";
import { PlaygroundPredictionOverlay } from "./PlaygroundPredictionOverlay";

export interface PlaygroundResultsPanelProps {
  doneJobs: Job[];
  activeJobs: Job[];
  failedJobs: Job[];
  selectedJobId: string | null;
  onSelectJob: (id: string) => void;
  predictions: PredictionsData | null;
  imagesMap: Record<string, { url: string; width: number; height: number }>;
  loadingPredictions: boolean;
  overlayStats: {
    total: number;
    withDetections: number;
    totalBoxes: number;
    skips: number;
  } | null;
  datasetClassesCache: React.RefObject<
    Record<string, { name: string; color: string }[]>
  >;
  jobs: Job[];
  onRefreshJobs: () => void;
  onDownloadPredictions: (jobId: string) => void;
}

export function PlaygroundResultsPanel({
  doneJobs,
  activeJobs,
  failedJobs,
  selectedJobId,
  onSelectJob,
  predictions,
  imagesMap,
  loadingPredictions,
  overlayStats,
  datasetClassesCache,
  jobs,
  onRefreshJobs,
  onDownloadPredictions,
}: PlaygroundResultsPanelProps) {
  const router = useRouter();

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Cabeçalho dos resultados */}
      <div className="flex shrink-0 items-center justify-between border-b border-white/5 px-4 py-3 md:px-5">
        <div className="flex items-center space-x-2">
          <IconTarget className="size-4 text-brand-400" />
          <span className="font-display text-sm font-semibold text-zinc-200">
            Resultados
          </span>
          {doneJobs.length > 0 && (
            <span className="rounded-full border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.5 font-mono text-2xs text-brand-300">
              {doneJobs.length}
            </span>
          )}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={onRefreshJobs}
          title="Recarregar jobs"
        >
          <IconRefresh className="size-3.5" />
        </Button>
      </div>

      {/* Conteúdo dos resultados */}
      <div className="flex-1 overflow-y-auto p-4 md:p-5">
        {/* Jobs ativos */}
        {activeJobs.length > 0 && (
          <div className="mb-4">
            <h3 className="mb-2 font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
              Em execução
            </h3>
            <div className="space-y-2">
              {activeJobs.map((job) => (
                <GlassCard
                  key={job.id}
                  className="flex items-center justify-between p-3"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="relative flex h-2 w-2 shrink-0">
                        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-brand-400 opacity-75 motion-reduce:animate-none" />
                        <span className="relative inline-flex h-2 w-2 rounded-full bg-brand-400" />
                      </span>
                      <span className="truncate font-mono text-xs text-zinc-200">
                        {job.model || "predict"}
                      </span>
                      <span className="rounded border border-zinc-800 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-3xs uppercase tracking-wider text-zinc-400">
                        {job.status}
                      </span>
                    </div>
                    {job.queuePosition != null && job.queuePosition > 0 && (
                      <p className="mt-0.5 pl-4 font-mono text-2xs text-zinc-500">
                        Posição na fila: {job.queuePosition}
                      </p>
                    )}
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => router.push(`/jobs?job=${job.id}`)}
                  >
                    Ver
                  </Button>
                </GlassCard>
              ))}
            </div>
          </div>
        )}

        {/* Jobs failed */}
        {failedJobs.length > 0 && (
          <div className="mb-4">
            <h3 className="mb-2 font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-red-400">
              Falhou
            </h3>
            <div className="space-y-2">
              {failedJobs.map((job) => (
                <GlassCard
                  key={job.id}
                  className="flex items-center justify-between p-3"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="relative flex h-2 w-2 shrink-0">
                        <span className="relative inline-flex h-2 w-2 rounded-full bg-red-400" />
                      </span>
                      <span className="truncate font-mono text-xs text-zinc-200">
                        {job.model || "predict"}
                      </span>
                      <span className="rounded border border-red-800/40 bg-red-900/30 px-1.5 py-0.5 font-mono text-3xs uppercase tracking-wider text-red-400">
                        failed
                      </span>
                    </div>
                    {job.queueReason && (
                      <p className="mt-0.5 pl-4 font-mono text-2xs text-zinc-500 truncate">
                        {job.queueReason}
                      </p>
                    )}
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => router.push(`/jobs?job=${job.id}`)}
                  >
                    Ver em Execuções
                  </Button>
                </GlassCard>
              ))}
            </div>
          </div>
        )}

        {/* Lista de jobs predict concluídos */}
        {doneJobs.length > 0 && (
          <div className="mb-4">
            <h3 className="mb-2 font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
              Concluídos
            </h3>
            <div className="space-y-1.5">
              {doneJobs.map((job) => (
                <div
                  key={job.id}
                  className={`group flex w-full items-center justify-between gap-2 rounded-lg border px-3 py-2 text-left transition-colors ${
                    selectedJobId === job.id
                      ? "border-brand-500/30 bg-brand-500/15"
                      : "border-transparent hover:bg-white/[0.04]"
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => onSelectJob(job.id)}
                    aria-pressed={selectedJobId === job.id}
                    className="flex min-w-0 flex-1 cursor-pointer items-center justify-between gap-2 rounded-md text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
                  >
                    <div className="min-w-0 flex-1">
                      <span className="truncate font-mono text-xs text-zinc-200">
                        {job.model || "predict"}
                      </span>
                      <span className="ml-2 font-mono text-3xs text-zinc-500">
                        {new Date(job.createdAt).toLocaleDateString("pt-BR", {
                          day: "2-digit",
                          month: "2-digit",
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </span>
                    </div>
                  </button>
                  <div className="flex items-center gap-1.5">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="opacity-0 group-hover:opacity-100"
                      onClick={() => {
                        onDownloadPredictions(job.id);
                      }}
                      title="Baixar predictions.json"
                    >
                      <IconDownload className="size-3.5" />
                    </Button>
                    <span className="rounded-full bg-status-success/10 px-1.5 py-0.5 font-mono text-3xs text-status-success">
                      done
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Loading predictions */}
        {loadingPredictions && (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <Spinner className="mb-3 size-8" />
            <p className="font-mono text-xs text-zinc-400">
              Carregando predições…
            </p>
          </div>
        )}

        {/* Overlay de predições */}
        {predictions && !loadingPredictions && (
          <div>
            {/* Estatísticas */}
            {overlayStats && (
              <div className="mb-4 flex flex-wrap gap-3">
                <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                  <span className="block font-mono text-3xs uppercase tracking-wider text-zinc-500">
                    Imagens
                  </span>
                  <span className="font-mono text-sm tabular-nums text-zinc-200">
                    {overlayStats.total}
                  </span>
                </div>
                <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                  <span className="block font-mono text-3xs uppercase tracking-wider text-zinc-500">
                    Com detecção
                  </span>
                  <span className="font-mono text-sm tabular-nums text-zinc-200">
                    {overlayStats.withDetections}
                  </span>
                </div>
                <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                  <span className="block font-mono text-3xs uppercase tracking-wider text-zinc-500">
                    Total boxes
                  </span>
                  <span className="font-mono text-sm tabular-nums text-brand-300">
                    {overlayStats.totalBoxes}
                  </span>
                </div>
                {overlayStats.skips > 0 && (
                  <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                    <span className="block font-mono text-3xs uppercase tracking-wider text-zinc-500">
                      Skips
                    </span>
                    <span className="font-mono text-sm tabular-nums text-amber-400">
                      {overlayStats.skips}
                    </span>
                  </div>
                )}
                <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                  <span className="block font-mono text-3xs uppercase tracking-wider text-zinc-500">
                    Conf
                  </span>
                  <span className="font-mono text-sm tabular-nums text-zinc-300">
                    ≥ {predictions.conf.toFixed(2)}
                  </span>
                </div>
              </div>
            )}

            {/* Download button */}
            {selectedJobId && (
              <div className="mb-4">
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => onDownloadPredictions(selectedJobId)}
                >
                  <IconDownload className="size-3.5" />
                  Baixar predictions.json
                </Button>
              </div>
            )}

            {/* Grid de imagens com overlay */}
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
              {predictions.images.map((predImg) => {
                const img = imagesMap[predImg.filename];
                const job = jobs.find((j) => j.id === selectedJobId);
                const classes = job?.datasetId
                  ? datasetClassesCache.current?.[job.datasetId]
                  : undefined;

                return (
                  <PlaygroundPredictionOverlay
                    key={predImg.filename}
                    prediction={predImg}
                    image={img}
                    classes={classes}
                  />
                );
              })}
            </div>
          </div>
        )}

        {/* Empty state: nenhum job ainda */}
        {!loadingPredictions &&
          doneJobs.length === 0 &&
          activeJobs.length === 0 &&
          failedJobs.length === 0 &&
          !predictions && (
            <div className="flex flex-col items-center justify-center py-20 text-center">
              <div className="mb-3 flex size-12 items-center justify-center rounded-2xl border border-brand-500/20 bg-brand-500/10 text-brand-400">
                <IconTarget className="size-6" />
              </div>
              <h3 className="font-display text-sm font-semibold text-zinc-200">
                Pronto para inferir
              </h3>
              <p className="mt-1 max-w-xs font-mono text-xs text-zinc-500">
                Selecione um modelo e dataset à esquerda e clique em Executar
                Detecção.
              </p>
            </div>
          )}
      </div>
    </div>
  );
}
