"use client";

import React from "react";

/**
 * Contadores de IMAGENS do batch de geração: `step` é 0-based da engine
 * (exibido 1-based como `step + 1`), `totalSteps` o tamanho do batch.
 * NÃO são training-steps/epochs do YOLO — nunca repassar telemetria de
 * treino sem conversão.
 */
export interface JobProgressLiveProps {
  jobKind?: string | null;
  phase?: string | null;
  phaseMessage?: string | null;
  progress?: number;
  vramUsedGb?: number | null;
  step?: number | null;
  totalSteps?: number | null;
  epoch?: number | null;
  totalEpochs?: number | null;
  isLive?: boolean;
  isFinished?: boolean;
  compact?: boolean;
  className?: string;
}

const PHASE_LABELS: Record<string, string> = {
  init: "Inicialização",
  preparing: "Preparando Ambiente",
  packaging_dataset: "Empacotando Dataset",
  downloading_dataset: "Sincronizando Dataset",
  extracting_dataset: "Extraindo Dataset",
  downloading: "Download de Pesos",
  downloading_weights: "Download de Pesos",
  starting_container: "Iniciando Nó GPU",
  loading_model: "Carregando Modelo",
  load_transformer: "Carregando Transformer",
  load_text_encoder: "Carregando Text Encoder",
  quantizing: "Quantização",
  quantizing_transformer: "Quantizando Transformer",
  quantizing_text_encoder: "Quantizando Text Encoder",
  setup_lora: "Configurando LoRA",
  injecting_lora: "Injeção de LoRA",
  dataset_ready: "Dataset Carregado",
  generating: "Gerando Imagem",
  generating_baseline_sample: "Amostra Baseline",
  generating_sample: "Gerando Amostra",
  sample_ready: "Amostra Concluída",
  baseline_ready: "Amostra Inicial Pronta",
  training: "Treinamento",
  training_started: "Treino Iniciado",
  epoch_complete: "Época Concluída",
  saving: "Gravando Artefato",
  completed: "Concluído",
  done: "Finalizado",
  error: "Falha na Execução",
  failed: "Erro",
  cancelled: "Cancelado",
};

export function JobProgressLive({
  jobKind,
  phase,
  phaseMessage,
  progress = 0,
  vramUsedGb,
  step,
  totalSteps,
  epoch,
  totalEpochs,
  isLive = false,
  isFinished = false,
  compact = false,
  className = "",
}: JobProgressLiveProps) {
  const normProgress = Math.max(0, Math.min(1, progress || 0));
  const percent = Math.round(normProgress * 100);

  const rawPhase = phase?.toLowerCase() || (isFinished ? "completed" : "preparing");
  const isGeneratingSample = rawPhase === "generating_sample" || rawPhase === "generating_baseline_sample";
  const isGenerationJob = jobKind === "diffusion_generate" || jobKind === "diffusion";
  const displayPhase =
    isGeneratingSample
      ? "Gerando Amostra"
      : isGenerationJob && rawPhase === "generating"
        ? "Gerando Imagem"
        : PHASE_LABELS[rawPhase] || rawPhase.replace(/_/g, " ").toUpperCase();
  const isError = rawPhase === "error" || rawPhase === "failed";
  if (compact) {
    return (
      <div className={`space-y-1.5 ${className}`}>
        <div className="flex items-center justify-between text-xs">
          <div className="flex items-center gap-2 min-w-0">
            {isLive && !isFinished && (
              <span className="relative flex h-2 w-2">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-violet-400 opacity-75" />
                <span className="relative inline-flex rounded-full h-2 w-2 bg-violet-500" />
              </span>
            )}
            <span className="font-mono font-medium text-zinc-200 truncate">
              {displayPhase}
            </span>
            {vramUsedGb !== undefined && vramUsedGb !== null && vramUsedGb > 0 && (
              <span className="font-mono text-3xs text-zinc-400 bg-white/5 px-1.5 py-0.5 rounded border border-white/10">
                {vramUsedGb.toFixed(1)} GB VRAM
              </span>
            )}
          </div>
          <span className="font-mono text-zinc-300 font-semibold">{percent}%</span>
        </div>

        <div className="relative h-1.5 w-full bg-zinc-900/80 rounded-full overflow-hidden border border-white/5">
          <div
            className={`h-full transition-all duration-300 ease-out ${
              isError
                ? "bg-rose-500"
                : "bg-gradient-to-r from-violet-600 via-brand-500 to-indigo-500"
            }`}
            style={{ width: `${percent}%` }}
          />
        </div>

        {phaseMessage && (
          <p className="text-2xs text-zinc-400 truncate">{phaseMessage}</p>
        )}
      </div>
    );
  }

  return (
    <div
      className={`relative overflow-hidden rounded-xl border border-white/10 bg-zinc-950/70 p-4 shadow-xl backdrop-blur-md ${className}`}
    >
      {/* Top Header */}
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-center gap-2.5 min-w-0">
          {/* Status Indicator Dot */}
          <div className="relative flex h-2.5 w-2.5 flex-shrink-0 items-center justify-center">
            {isLive && !isFinished ? (
              <>
                <span className="absolute h-full w-full animate-ping rounded-full bg-violet-400 opacity-75" />
                <span className="relative h-2 w-2 rounded-full bg-violet-500" />
              </>
            ) : isError ? (
              <span className="h-2 w-2 rounded-full bg-rose-500" />
            ) : isFinished ? (
              <span className="h-2 w-2 rounded-full bg-violet-400" />
            ) : (
              <span className="h-2 w-2 rounded-full bg-zinc-600" />
            )}
          </div>

          {/* Phase Badge */}
          <span className="font-mono text-xs font-semibold uppercase tracking-wider text-violet-300 truncate">
            {displayPhase}
          </span>

          {/* VRAM Pill */}
          {vramUsedGb !== undefined && vramUsedGb !== null && vramUsedGb > 0 && (
            <span className="inline-flex items-center gap-1 rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-0.5 font-mono text-2xs font-medium text-zinc-300">
              <span className="h-1.5 w-1.5 rounded-full bg-violet-400" />
              {vramUsedGb.toFixed(1)} GB VRAM
            </span>
          )}

          {/* Badges de progresso contextual */}
          {isGeneratingSample && step !== null && step !== undefined && step > 0 && (
            <span className="inline-flex items-center gap-1 font-mono text-2xs text-amber-300 bg-amber-500/10 border border-amber-500/20 px-2 py-0.5 rounded animate-pulse">
              <span className="size-1.5 rounded-full bg-amber-400" />
              Amostra: Passo {step}{totalSteps ? `/${totalSteps}` : "/20"}
            </span>
          )}
          {!isGeneratingSample && isGenerationJob && step !== null && step !== undefined && totalSteps !== null && totalSteps !== undefined && totalSteps > 0 && (
            <span className="hidden sm:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded">
              Imagem {step + 1}/{totalSteps}
            </span>
          )}
          {!isGeneratingSample && !isGenerationJob && epoch !== null && epoch !== undefined && epoch > 0 && (
            <span className="hidden sm:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded">
              Época {epoch}
              {totalEpochs ? `/${totalEpochs}` : ""}
            </span>
          )}
          {!isGeneratingSample && !isGenerationJob && step !== null && step !== undefined && step > 0 && (
            <span className="hidden md:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded">
              Passo {step}{totalSteps ? `/${totalSteps}` : ""}
            </span>
          )}
        </div>

        {/* Progress Percentage */}
        <div className="font-mono text-sm font-bold text-white tabular-nums">
          {percent}%
        </div>
      </div>

      {/* Phase Message */}
      {phaseMessage && (
        <p className="mt-2 text-xs text-zinc-400 truncate">
          {phaseMessage}
        </p>
      )}

      {/* Progress Bar Track */}
      <div className="relative mt-3 h-2 w-full overflow-hidden rounded-full border border-white/5 bg-zinc-900/90">
        <div
          className={`h-full transition-all duration-300 ease-out ${
            isError
              ? "bg-rose-500"
              : "bg-gradient-to-r from-violet-600 via-brand-500 to-indigo-500 shadow-[0_0_12px_rgba(139,92,246,0.3)]"
          }`}
          style={{ width: `${percent}%` }}
        />
        {/* Shimmer animation on active progress */}
        {isLive && !isFinished && (
          <div className="absolute inset-0 bg-gradient-to-r from-transparent via-white/15 to-transparent animate-shimmer" />
        )}
      </div>
    </div>
  );
}
