"use client";

import { useRef } from "react";
import { formatDurationMs } from "@/lib/format";
import { estimateTrainingEtaMs } from "@/lib/jobMetrics";
import type { JobTelemetryEvent } from "@/types/studio";

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
  vramReservedGb?: number | null;
  stepTimeSeconds?: number | null;
  speed?: string | null;
  etaSeconds?: number | null;
  etaFormatted?: string | null;
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
  preparing_dataset: "Preparando Dataset",
  preparing_cache: "Pré-computando Cache",
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
  vramReservedGb,
  stepTimeSeconds,
  speed,
  etaSeconds,
  etaFormatted,
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

  const isRunning = isLive && !isFinished;

  const hasUsed = vramUsedGb !== undefined && vramUsedGb !== null && vramUsedGb > 0;
  const hasReserved = vramReservedGb !== undefined && vramReservedGb !== null && vramReservedGb > 0;
  const hasVram = hasUsed || hasReserved;
  const vramLabel =
    hasUsed && hasReserved
      ? `${vramUsedGb?.toFixed(1)} / ${vramReservedGb?.toFixed(1)} GB VRAM`
      : hasUsed
        ? `${vramUsedGb?.toFixed(1)} GB VRAM`
        : hasReserved
          ? `${vramReservedGb?.toFixed(1)} GB VRAM`
          : null;
  const vramTooltip =
    hasUsed && hasReserved
      ? `VRAM: ${vramUsedGb?.toFixed(2)} GB alocada / ${vramReservedGb?.toFixed(2)} GB reservada`
      : hasUsed
        ? `VRAM: ${vramUsedGb?.toFixed(2)} GB alocada`
        : hasReserved
          ? `VRAM: ${vramReservedGb?.toFixed(2)} GB reservada`
          : undefined;

  const speedLabel = speed
    ? speed
    : typeof stepTimeSeconds === "number" && Number.isFinite(stepTimeSeconds) && stepTimeSeconds > 0
      ? `${stepTimeSeconds.toFixed(1)}s/step`
      : null;

  /* F1 — janela rolante p/ ETA: o componente recebe snapshots, não o stream;
     acumula amostras (step/totalSteps/phase + wall-clock) num buffer limitado.
     Step retrocedendo = job novo reutilizando o card → zera o buffer. */
  const samplesRef = useRef<JobTelemetryEvent[]>([]);
  if (
    isRunning &&
    typeof phase === "string" &&
    typeof step === "number" &&
    Number.isFinite(step) &&
    typeof totalSteps === "number" &&
    Number.isFinite(totalSteps) &&
    totalSteps > 0
  ) {
    const buf = samplesRef.current;
    const prev = buf.length > 0 ? buf[buf.length - 1] : undefined;
    if (!prev || prev.step !== step || prev.phase !== phase) {
      if (prev && typeof prev.step === "number" && step < prev.step)
        buf.length = 0;
      buf.push({
        timestamp: new Date().toISOString(),
        phase,
        phaseMessage: phaseMessage ?? null,
        progress: normProgress,
        step,
        totalSteps,
        epoch: epoch ?? null,
        totalEpochs: totalEpochs ?? null,
        vramUsedGb: vramUsedGb ?? null,
        vramReservedGb: vramReservedGb ?? null,
        stepTimeSeconds: stepTimeSeconds ?? null,
        speed: speed ?? null,
        etaSeconds: etaSeconds ?? null,
        etaFormatted: etaFormatted ?? null,
      });
      if (buf.length > 64) buf.splice(0, buf.length - 64);
    }
  }
  const clientEtaMs = isRunning ? estimateTrainingEtaMs(samplesRef.current) : null;
  let displayEta: string | null = null;
  if (isRunning) {
    if (etaFormatted && etaFormatted.trim().length > 0) {
      const clean = etaFormatted.trim();
      displayEta = clean.toLowerCase().startsWith("eta") ? clean : `ETA ~${clean}`;
    } else if (typeof etaSeconds === "number" && Number.isFinite(etaSeconds) && etaSeconds >= 0) {
      displayEta = `ETA ~${formatDurationMs(etaSeconds * 1000)}`;
    } else if (
      clientEtaMs !== null &&
      typeof step === "number" &&
      typeof totalSteps === "number" &&
      step < totalSteps
    ) {
      displayEta = `ETA ~${formatDurationMs(clientEtaMs)}`;
    }
  }
  const showEta = displayEta !== null;
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
            {hasVram && vramLabel && (
              <span
                className="font-mono text-3xs text-zinc-400 bg-white/5 px-1.5 py-0.5 rounded border border-white/10 whitespace-nowrap flex-shrink-0"
                title={vramTooltip}
              >
                {vramLabel}
              </span>
            )}
            {speedLabel && !isFinished && (
              <span
                className="hidden sm:inline-block font-mono text-3xs text-zinc-400 bg-white/5 px-1.5 py-0.5 rounded border border-white/10 tabular-nums whitespace-nowrap flex-shrink-0"
                title="Velocidade"
              >
                {speedLabel}
              </span>
            )}
          </div>
          <span className="flex items-baseline gap-1.5 flex-shrink-0">
            {showEta && displayEta && (
              <span className="font-mono text-3xs text-zinc-500 tabular-nums whitespace-nowrap">
                {displayEta}
              </span>
            )}
            <span className="font-mono text-zinc-300 font-semibold">{percent}%</span>
          </span>
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
        <div className="flex items-center gap-2 min-w-0 flex-wrap">
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
          <span className="font-mono text-xs font-semibold uppercase tracking-wider text-violet-300 whitespace-nowrap flex-shrink-0">
            {displayPhase}
          </span>

          {/* VRAM Pill */}
          {hasVram && vramLabel && (
            <span
              className="inline-flex items-center gap-1 rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-0.5 font-mono text-2xs font-medium text-zinc-300 whitespace-nowrap flex-shrink-0"
              title={vramTooltip}
            >
              <span className="h-1.5 w-1.5 rounded-full bg-violet-400" />
              {vramLabel}
            </span>
          )}

          {/* Speed Badge */}
          {speedLabel && !isFinished && (
            <span
              className="hidden sm:inline-flex items-center gap-1 font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded tabular-nums whitespace-nowrap flex-shrink-0"
              title="Velocidade de processamento"
            >
              {speedLabel}
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
            <span className="hidden sm:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded whitespace-nowrap flex-shrink-0">
              Imagem {step + 1}/{totalSteps}
            </span>
          )}
          {!isGeneratingSample && !isGenerationJob && epoch !== null && epoch !== undefined && epoch > 0 && (
            <span className="hidden sm:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded whitespace-nowrap flex-shrink-0">
              Época {epoch}
              {totalEpochs ? `/${totalEpochs}` : ""}
            </span>
          )}
          {!isGeneratingSample && !isGenerationJob && step !== null && step !== undefined && step > 0 && (
            <span className="hidden md:inline-block font-mono text-2xs text-zinc-400 bg-white/[0.03] border border-white/5 px-2 py-0.5 rounded whitespace-nowrap flex-shrink-0">
              Passo {step}{totalSteps ? `/${totalSteps}` : ""}
            </span>
          )}
        </div>

        {/* Progress Percentage + ETA */}
        <div className="flex items-baseline gap-2 shrink-0 ml-auto pl-2">
          {showEta && displayEta && (
            <span
              className="font-mono text-2xs font-medium text-zinc-400 tabular-nums whitespace-nowrap"
              title="Tempo estimado restante de treino"
            >
              {displayEta}
            </span>
          )}
          <div className="font-mono text-sm font-bold text-white tabular-nums">
            {percent}%
          </div>
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
