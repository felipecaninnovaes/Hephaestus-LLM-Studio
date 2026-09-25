"use client";

import {
  IconDownload,
  IconSparkles,
  IconZoomIn,
} from "@/components/icons";
import { JobProgressLive } from "@/components/studio/JobProgressLive";
import type { UseJobTelemetryReturn } from "@/hooks/useJobTelemetry";
import { Button } from "@/components/ui/Button";
import { GlassCard } from "@/components/ui/GlassCard";
import { Kbd } from "@/components/ui/Kbd";
import type { Job } from "@/types/jobs";
import {
  type GeneratedImageItem,
  UPSCALE_MODEL_SHORT_LABEL,
} from "./generationTypes";

export interface GenerationPreviewCardProps {
  activeJobId: string | null;
  activeJob: Job | null;
  telemetry: UseJobTelemetryReturn;
  currentDisplayItem: GeneratedImageItem | null;
  batchResults: GeneratedImageItem[];
  history: GeneratedImageItem[];
  onSelectDisplayItem: (item: GeneratedImageItem) => void;
  onDownload: () => void;
  onOpenLightbox: () => void;
}

export function GenerationPreviewCard({
  activeJobId,
  activeJob,
  telemetry,
  currentDisplayItem,
  batchResults,
  history,
  onSelectDisplayItem,
  onDownload,
  onOpenLightbox,
}: GenerationPreviewCardProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-visible p-4 md:p-5 lg:overflow-hidden">
      {/* Telemetria live */}
      {activeJobId && (
        <div className="mb-4">
          <JobProgressLive
            jobKind="diffusion_generate"
            phase={telemetry.phase || activeJob?.phase || activeJob?.status}
            phaseMessage={
              telemetry.phaseMessage ||
              activeJob?.phaseMessage ||
              (activeJob?.status === "running" ? "Gerando imagem…" : "Na fila…")
            }
            progress={telemetry.progress || activeJob?.progress || 0}
            vramUsedGb={telemetry.vramUsedGb ?? activeJob?.vramUsedGb}
            vramReservedGb={telemetry.vramReservedGb ?? activeJob?.vramReservedGb}
            stepTimeSeconds={telemetry.stepTimeSeconds}
            speed={telemetry.speed}
            etaSeconds={telemetry.etaSeconds}
            etaFormatted={telemetry.etaFormatted}
            step={telemetry.step ?? activeJob?.step}
            totalSteps={telemetry.totalSteps ?? activeJob?.totalSteps ?? null}
            isLive={telemetry.isLive}
            isFinished={telemetry.isFinished}
          />
        </div>
      )}

      {/* Canvas / Resultado */}
      {currentDisplayItem ? (
        <GlassCard className="flex-1 min-h-0 flex flex-col p-4 border-white/10">
          {/* Toolbar */}
          <div className="flex flex-wrap items-center justify-between gap-2 pb-3 border-b border-white/5 mb-3">
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold text-zinc-200">
                Resultado
              </span>
              <span className="font-mono text-3xs px-2 py-0.5 rounded bg-zinc-800 border border-white/5 text-zinc-400">
                {currentDisplayItem.width}×{currentDisplayItem.height}
              </span>
              <span className="font-mono text-3xs px-2 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                seed {currentDisplayItem.seed}
              </span>
            </div>
            <div className="flex items-center gap-1.5">
              <Button
                size="sm"
                variant="ghost"
                onClick={onDownload}
                title="Baixar PNG"
              >
                <IconDownload className="size-3.5" />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={onOpenLightbox}
                title="Ampliar"
              >
                <IconZoomIn className="size-3.5" />
              </Button>
            </div>
          </div>

          {/* Imagem */}
          <div className="flex-1 min-h-0 flex items-center justify-center rounded-xl overflow-hidden bg-black/40 border border-white/5">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={currentDisplayItem.imageUrl}
              alt={currentDisplayItem.prompt}
              className="max-h-full max-w-full object-contain"
            />
          </div>

          {/* Metadados */}
          <div className="mt-3 p-2.5 rounded-lg border border-white/5 bg-zinc-950/60">
            <p
              className="text-2xs text-zinc-300 italic leading-relaxed truncate"
              title={currentDisplayItem.prompt}
            >
              &ldquo;{currentDisplayItem.prompt}&rdquo;
            </p>
            <div className="flex flex-wrap items-center gap-1.5 mt-1.5">
              <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                {currentDisplayItem.baseModel}
              </span>
              <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                steps {currentDisplayItem.steps}
              </span>
              <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                CFG {currentDisplayItem.guidanceScale.toFixed(1)}
              </span>
              <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                {currentDisplayItem.quantization}
              </span>
              {currentDisplayItem.sampler &&
                currentDisplayItem.sampler !== "default" && (
                  <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                    {currentDisplayItem.sampler}
                  </span>
                )}
              {currentDisplayItem.upscale && (
                <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                  upscale{" "}
                  {UPSCALE_MODEL_SHORT_LABEL[
                    currentDisplayItem.upscale.model
                  ] ?? currentDisplayItem.upscale.model}{" "}
                  {currentDisplayItem.upscale.scale}x
                </span>
              )}
              {currentDisplayItem.loras.length > 0 && (
                <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                  {currentDisplayItem.loras.length} LoRA(s)
                </span>
              )}
            </div>
          </div>

          {/* Batch results grid */}
          {batchResults.length > 1 && (
            <div className="mt-3 space-y-2">
              <span className="font-mono text-3xs text-zinc-400 uppercase tracking-wider">
                Batch ({batchResults.length} imagens)
              </span>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                {batchResults.map((item) => (
                  <button
                    key={`${item.jobId}-${item.seed}`}
                    type="button"
                    onClick={() => onSelectDisplayItem(item)}
                    className={`group relative rounded-lg overflow-hidden border aspect-square transition-all cursor-pointer ${
                      currentDisplayItem?.batchIndex === item.batchIndex &&
                      currentDisplayItem?.jobId === item.jobId
                        ? "border-brand-500 ring-2 ring-brand-500/30"
                        : "border-white/10 hover:border-white/20 opacity-70 hover:opacity-100"
                    }`}
                  >
                    {/* eslint-disable-next-line @next/next/no-img-element */}
                    <img
                      src={item.imageUrl}
                      alt={`seed ${item.seed}`}
                      className="w-full h-full object-cover"
                    />
                    <div className="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/80 to-transparent p-1">
                      <span className="font-mono text-[8px] text-zinc-300">
                        #{item.seed}
                      </span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}
        </GlassCard>
      ) : (
        <GlassCard className="flex-1 flex flex-col items-center justify-center p-12 border-white/10 text-center space-y-3">
          <div className="w-12 h-12 rounded-2xl border border-brand-500/20 bg-brand-500/[0.08] flex items-center justify-center text-brand-400">
            <IconSparkles className="size-6" />
          </div>
          <h3 className="text-sm font-semibold text-zinc-200">
            Pronto para gerar
          </h3>
          <p className="text-xs text-zinc-400 max-w-sm">
            Configure os parâmetros à esquerda e clique &ldquo;Gerar&rdquo; ou
            pressione <Kbd className="mx-0.5">Ctrl+Enter</Kbd>.
          </p>
        </GlassCard>
      )}

      {/* Histórico de sessão */}
      {history.length > 0 && (
        <div className="mt-4 shrink-0">
          <div className="flex items-center justify-between mb-2">
            <span className="font-mono text-3xs text-zinc-400 uppercase tracking-wider">
              Sessão ({history.length})
            </span>
          </div>
          <div className="flex gap-2 overflow-x-auto pb-1">
            {history.slice(0, 20).map((item, idx) => {
              const isSelected =
                currentDisplayItem?.jobId === item.jobId &&
                currentDisplayItem?.batchIndex === item.batchIndex;
              return (
                <button
                  key={`${item.jobId}-${item.batchIndex ?? idx}`}
                  type="button"
                  onClick={() => onSelectDisplayItem(item)}
                  className={`shrink-0 size-14 rounded-lg overflow-hidden border transition-all cursor-pointer ${
                    isSelected
                      ? "border-brand-500 ring-2 ring-brand-500/30"
                      : "border-white/10 opacity-60 hover:opacity-100"
                  }`}
                >
                  {/* eslint-disable-next-line @next/next/no-img-element */}
                  <img
                    src={item.thumbUrl || item.imageUrl}
                    alt=""
                    className="w-full h-full object-cover"
                  />
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
