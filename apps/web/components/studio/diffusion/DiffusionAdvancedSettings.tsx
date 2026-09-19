"use client";

import { useMemo, useState } from "react";
import { IconChevronDown, IconChevronRight } from "@/components/icons";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption } from "@/components/ui/Select";
import type { DiffusionOptimizer } from "@/types/studio";
import type { DiffusionBaseModel } from "./estimateDiffusionVram";

export interface DiffusionAdvancedSettingsProps {
  baseModel: DiffusionBaseModel;
  learningRate: string;
  onLearningRateChange: (lr: string) => void;
  quantization: "none" | "2bit" | "4bit" | "6bit" | "8bit";
  setQuantization: (val: "none" | "2bit" | "4bit" | "6bit" | "8bit") => void;
  resolution: number;
  setResolution: (val: number) => void;
  enableBucket: boolean;
  setEnableBucket: (val: boolean) => void;
  gradientAccumulationSteps: number;
  setGradientAccumulationSteps: (val: number) => void;
  optimizer: DiffusionOptimizer;
  setOptimizer: (val: DiffusionOptimizer) => void;
  lrScheduler: "cosine" | "linear" | "constant" | "constant_with_warmup";
  setLrScheduler: (
    val: "cosine" | "linear" | "constant" | "constant_with_warmup",
  ) => void;
  lrWarmupSteps: number;
  setLrWarmupSteps: (val: number) => void;
  mixedPrecision: "fp16" | "bf16" | "no";
  setMixedPrecision: (val: "fp16" | "bf16" | "no") => void;
  checkpointInterval: number;
  setCheckpointInterval: (val: number) => void;
  busy: boolean;
}

export function DiffusionAdvancedSettings({
  baseModel,
  learningRate,
  onLearningRateChange,
  quantization,
  setQuantization,
  resolution,
  setResolution,
  enableBucket,
  setEnableBucket,
  gradientAccumulationSteps,
  setGradientAccumulationSteps,
  optimizer,
  setOptimizer,
  lrScheduler,
  setLrScheduler,
  lrWarmupSteps,
  setLrWarmupSteps,
  mixedPrecision,
  setMixedPrecision,
  checkpointInterval,
  setCheckpointInterval,
  busy,
}: DiffusionAdvancedSettingsProps) {
  const [showAdvanced, setShowAdvanced] = useState(false);

  const resolutionOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 256, label: "256 x 256 (Miniatura / Menor VRAM)" },
      { value: 512, label: "512 x 512 (Padrão SD 1.5 / Menor VRAM)" },
      { value: 768, label: "768 x 768 (Intermediário)" },
      { value: 1024, label: "1024 x 1024 (Padrão SDXL / FLUX)" },
      { value: 1280, label: "1280 x 1280 (Alta fidelidade)" },
      { value: 1328, label: "1328 x 1328 (Panorâmica SDXL)" },
      { value: 1536, label: "1536 x 1536 (Ultra detalhe)" },
      { value: 2048, label: "2048 x 2048 (Máximo — Alta VRAM)" },
    ],
    [],
  );

  const gradAccumOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 1, label: "1x (Atualização a cada batch)" },
      { value: 2, label: "2x (Batch efetivo 2x sem VRAM extra)" },
      { value: 4, label: "4x (Batch efetivo 4x sem VRAM extra)" },
      { value: 8, label: "8x (Batch efetivo 8x sem VRAM extra)" },
    ],
    [],
  );

  const optimizerOptions = useMemo<SelectOption<DiffusionOptimizer>[]>(
    () => [
      {
        value: "paged_adamw8bit",
        label: "Paged AdamW 8-bit (BitsAndBytes — Recomendado p/ QLoRA)",
      },
      {
        value: "paged_adamw32bit",
        label: "Paged AdamW 32-bit (BitsAndBytes — Máxima precisão)",
      },
      {
        value: "adamw8bit",
        label: "AdamW 8-bit (BitsAndBytes Padrão)",
      },
      {
        value: "adamw",
        label: "AdamW FP32 (Padrão PyTorch)",
      },
      {
        value: "prodigy",
        label: "Prodigy (Taxa adaptativa D-Adaptation)",
      },
    ],
    [],
  );

  const lrSchedulerOptions = useMemo<
    SelectOption<"cosine" | "linear" | "constant" | "constant_with_warmup">[]
  >(
    () => [
      { value: "cosine", label: "Cosine (Decaimento suave em cosseno)" },
      { value: "linear", label: "Linear (Decaimento linear até zero)" },
      { value: "constant", label: "Constant (Taxa fixa sem decaimento)" },
      { value: "constant_with_warmup", label: "Constant com Warmup" },
    ],
    [],
  );

  const mixedPrecisionOptions = useMemo<SelectOption<"fp16" | "bf16" | "no">[]>(
    () => [
      { value: "fp16", label: "FP16 (Half - Padrão universal GPU)" },
      { value: "bf16", label: "BF16 (Bfloat16 - Ampere/Ada/Hopper)" },
      { value: "no", label: "Desativado (FP32 completo - Alto consumo VRAM)" },
    ],
    [],
  );

  const quantizationOptions = useMemo<
    SelectOption<"none" | "2bit" | "4bit" | "6bit" | "8bit">[]
  >(
    () => [
      {
        value: "2bit",
        label: "2-bit TorchAo (Mínima VRAM — degradação visível, p/ testes)",
      },
      {
        value: "4bit",
        label: "4-bit NF4 (BitsAndBytes — Recomendado p/ GPUs ≤ 12GB)",
      },
      { value: "6bit", label: "6-bit TorchAo (Intermediário — ~10 GB VRAM)" },
      {
        value: "8bit",
        label: "8-bit BitsAndBytes (Equilíbrio p/ GPUs ≥ 16GB)",
      },
      { value: "none", label: "Nenhum (FP16/BF16 Pleno — GPUs ≥ 24GB)" },
    ],
    [],
  );

  const checkpointIntervalOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 1, label: "A cada 1 época (Máxima segurança)" },
      { value: 2, label: "A cada 2 épocas" },
      { value: 5, label: "A cada 5 épocas (Recomendado)" },
      { value: 10, label: "A cada 10 épocas" },
    ],
    [],
  );

  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.01] transition-colors">
      <button
        type="button"
        aria-expanded={showAdvanced}
        aria-controls="advanced-diffusion-settings"
        onClick={() => setShowAdvanced((prev) => !prev)}
        className="w-full flex items-center justify-between p-3.5 text-left hover:bg-white/[0.02] transition-colors focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
      >
        <div className="flex items-center gap-2">
          {showAdvanced ? (
            <IconChevronDown className="size-4 text-brand-400" />
          ) : (
            <IconChevronRight className="size-4 text-zinc-400" />
          )}
          <span className="font-display text-xs font-semibold text-zinc-200">
            Configurações Avançadas de Treinamento
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-1.5 font-mono text-3xs text-zinc-400">
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            {resolution}x{resolution}
          </span>
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            GA: {gradientAccumulationSteps}x
          </span>
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            {optimizer === "paged_adamw8bit"
              ? "Paged 8-bit"
              : optimizer === "paged_adamw32bit"
                ? "Paged 32-bit"
                : optimizer === "adamw8bit"
                  ? "8-bit AdamW"
                  : optimizer === "prodigy"
                    ? "Prodigy"
                    : "AdamW"}
          </span>
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            {mixedPrecision.toUpperCase()}
          </span>
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            {quantization === "4bit"
              ? "4-BIT NF4"
              : quantization === "8bit"
                ? "8-BIT BNB"
                : quantization === "6bit"
                  ? "6-BIT TAO"
                  : quantization === "2bit"
                    ? "2-BIT TAO"
                    : "FP16 PLENO"}
          </span>
          <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
            {enableBucket ? "ASPECT RATIO" : "QUADRADO"}
          </span>
        </div>
      </button>

      {showAdvanced && (
        <div
          id="advanced-diffusion-settings"
          className="p-4 pt-2 border-t border-white/5 space-y-4"
        >
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {/* Quantização do Modelo Base */}
            <Select
              id="diffusion-quant"
              label="Quantização do Modelo Base"
              hint={
                baseModel === "flux"
                  ? "2-bit/4-bit permitem rodar em GPUs ≤ 12 GB. 6-bit intermediário (~10 GB), 8-bit exige ≥ 16 GB e Nenhum exige ≥ 24 GB."
                  : "Quantização do backbone para redução drástica de memória VRAM (2-bit só p/ testes)."
              }
              options={quantizationOptions}
              value={quantization}
              onChange={(val) =>
                setQuantization(
                  val as "none" | "2bit" | "4bit" | "6bit" | "8bit",
                )
              }
              disabled={busy}
              fontMono
              size="default"
            />

            {/* Resolução de Treinamento */}
            <Select
              id="diffusion-res"
              label="Resolução Base de Entrada"
              hint="Área alvo dos buckets. Com bucketing ativo, a proporção original das imagens é preservada."
              options={resolutionOptions}
              value={resolution}
              onChange={(val) => setResolution(Number(val))}
              disabled={busy}
              fontMono
              size="default"
            />

            {/* Bucketing por Aspect Ratio */}
            <div className="rounded-lg border border-white/10 bg-white/[0.02] p-3 space-y-2">
              <label
                htmlFor="enable-bucket-toggle"
                className="flex items-start gap-2 cursor-pointer select-none"
              >
                <input
                  id="enable-bucket-toggle"
                  type="checkbox"
                  checked={enableBucket}
                  onChange={(e) => setEnableBucket(e.target.checked)}
                  disabled={busy}
                  className="mt-0.5 size-4 rounded border-white/20 bg-white/5 text-brand-500 focus:ring-brand-500/30"
                />
                <span className="font-mono text-xs font-semibold text-zinc-200 leading-tight">
                  Bucketing por Aspect Ratio
                </span>
              </label>
              <p className="text-3xs leading-relaxed text-zinc-400 pl-6">
                Agrupa as imagens por proporção em buckets de resolução múltipla
                de 64 (área ≈ resolução²), evitando esticar tudo para o
                quadrado. Recomendado para datasets com fotos em
                retrato/paisagem.
              </p>
            </div>

            {/* Gradient Accumulation */}
            <Select
              id="diffusion-ga"
              label="Gradient Accumulation"
              hint="Estabiliza gradientes somando N passos antes da atualização de pesos."
              options={gradAccumOptions}
              value={gradientAccumulationSteps}
              onChange={(val) => setGradientAccumulationSteps(Number(val))}
              disabled={busy}
              fontMono
              size="default"
            />

            {/* Otimizador */}
            <Select
              id="diffusion-opt"
              label="Otimizador"
              hint={
                optimizer === "prodigy"
                  ? "Requer LR=1.0 para o ajuste automático de D-Adaptation."
                  : optimizer === "paged_adamw8bit"
                    ? "Paginação CUDA BitsAndBytes: elimina OOM paginando estados para a RAM quando necessário (recomendado p/ QLoRA)."
                    : optimizer === "paged_adamw32bit"
                      ? "Paginação CUDA em 32-bit: máxima precisão numérica com proteção contra OOM."
                      : "8-bit economiza ~2 GB de VRAM no estado do otimizador."
              }
              options={optimizerOptions}
              value={optimizer}
              onChange={(opt) => {
                setOptimizer(opt);
                if (opt === "prodigy" && learningRate === "0.0001") {
                  onLearningRateChange("1.0");
                } else if (opt !== "prodigy" && learningRate === "1.0") {
                  onLearningRateChange("0.0001");
                }
              }}
              disabled={busy}
              fontMono
              size="default"
            />

            {/* LR Scheduler */}
            <Select
              id="diffusion-sched"
              label="LR Scheduler"
              hint="Controla o decaimento da taxa de aprendizado ao longo dos steps."
              options={lrSchedulerOptions}
              value={lrScheduler}
              onChange={(val) => setLrScheduler(val)}
              disabled={busy}
              fontMono
              size="default"
            />

            {/* LR Warmup Steps */}
            <Input
              id="diffusion-warmup"
              label="Warmup Steps"
              hint="Passos iniciais de aquecimento para evitar choques no gradiente."
              type="number"
              min={0}
              max={1000}
              value={lrWarmupSteps}
              onChange={(e) =>
                setLrWarmupSteps(Math.max(0, parseInt(e.target.value, 10) || 0))
              }
              disabled={busy}
              fontMono
              className="h-[38px] text-xs font-mono"
            />

            {/* Mixed Precision */}
            <Select
              id="diffusion-prec"
              label="Precisão Mista"
              hint="FP16/BF16 reduz pela metade o consumo de VRAM e acelera o treino em Tensor Cores."
              options={mixedPrecisionOptions}
              value={mixedPrecision}
              onChange={(val) => setMixedPrecision(val)}
              disabled={busy}
              fontMono
              size="default"
            />

            {/* Intervalo de Checkpoints */}
            <Select
              id="diffusion-checkpoint-interval"
              label="Intervalo de Checkpoints"
              hint="Frequência de upload ao vivo e gravação de snapshots (.safetensors)."
              options={checkpointIntervalOptions}
              value={checkpointInterval}
              onChange={(val) => setCheckpointInterval(Number(val))}
              disabled={busy}
              fontMono
              size="default"
            />
          </div>
        </div>
      )}
    </div>
  );
}
