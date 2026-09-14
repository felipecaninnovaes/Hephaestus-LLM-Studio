"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import {
  IconPlay,
  IconDatabase,
  IconAlertTriangle,
  IconZap,
  IconSettings,
  IconCpu,
  IconImage,
  IconDownload,
  IconUpload,
  IconChevronDown,
  IconChevronRight,
  IconSparkles,
  IconX,
} from "@/components/icons";
import { Button, getButtonClasses } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
import { getTelemetry, startDiffusionJob } from "@/lib/jobs";
import { listDatasets, canTrainDiffusion, trainDiffusionDisabledReason } from "@/lib/datasets";
import { listModels } from "@/lib/models";
import { formatBytes } from "@/lib/format";
import { ApiError } from "@/lib/api";
import { diffusionErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import NodeSelect from "./NodeSelect";
import type { Dataset, Model, Telemetry, DiffusionPreset } from "@/types/studio";

export const DIFFUSION_EPOCHS_MIN = 1;
export const DIFFUSION_EPOCHS_MAX = 100;

export type DiffusionBaseModel = "sdxl" | "flux" | "sd15";

export interface DiffusionHyperparametersValues {
  baseModel: DiffusionBaseModel;
  triggerWord: string;
  epochs: number;
  batchSize: number;
  learningRate: string;
  rank: number;
  alpha: number;
}

/**
 * Estimativa preditiva de VRAM em GB para treino de difusão LoRA.
 * Base: SD1.5 (8 GB), SDXL (12 GB), Flux (16 GB).
 * Fator de Batch, Rank, Resolução e Otimizador adicionam overhead ou economia.
 */
export function estimateDiffusionVramGb(
  baseModel: DiffusionBaseModel,
  batchSize: number,
  rank: number,
  resolution: number = 1024,
  optimizer: "adamw8bit" | "adamw" | "prodigy" = "adamw8bit",
  mixedPrecision: "fp16" | "bf16" | "no" = "fp16",
  quantization: "none" | "4bit" | "8bit" = "4bit",
): number {
  let baseGb = 12.0;
  if (baseModel === "sd15") {
    baseGb = quantization === "4bit" ? 6.0 : quantization === "8bit" ? 7.0 : 8.0;
  } else if (baseModel === "sdxl") {
    baseGb = quantization === "4bit" ? 9.5 : quantization === "8bit" ? 11.0 : 12.0;
  } else if (baseModel === "flux") {
    baseGb = quantization === "4bit" ? 10.0 : quantization === "8bit" ? 14.5 : 22.0;
  }

  // Ajuste por resolução relativa a 1024
  if (resolution <= 512) {
    baseGb -= baseModel === "sd15" ? 2.0 : 3.0;
  } else if (resolution <= 768) {
    baseGb -= baseModel === "sd15" ? 1.0 : 1.5;
  }

  const batchMemory = (batchSize - 1) * (baseModel === "flux" ? 1.8 : baseModel === "sdxl" ? 2.0 : 1.2);
  const rankMemory = (rank / 64) * 0.8;
  const optimMemory = optimizer === "adamw" ? 1.5 : optimizer === "prodigy" ? 0.6 : 0;
  const precMemory = mixedPrecision === "no" ? 3.5 : 0;

  return Math.max(4.0, Math.round((baseGb + batchMemory + rankMemory + optimMemory + precMemory) * 10) / 10);
}

interface Props {
  onJobCreated?: (jobId: string) => void;
  initialPreset?: Partial<DiffusionPreset>;
  resumeCheckpoint?: { id: string; name: string; epoch?: number } | null;
  epochOffset?: number;
}

export default function ForjaDifusaoSetup({
  onJobCreated,
  initialPreset,
  resumeCheckpoint,
  epochOffset: propEpochOffset = 0,
}: Props) {
  const firstRef = useRef<SelectRefHandle>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Datasets
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [datasetsLoading, setDatasetsLoading] = useState(true);
  const [selectedDatasetId, setSelectedDatasetId] = useState<string>("");

  // Models (fine-tune previous LoRA weights)
  const [diffusionModels, setDiffusionModels] = useState<Model[]>([]);
  const [selectedWeightId, setSelectedWeightId] = useState<string>(
    resumeCheckpoint?.id ?? ""
  );
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);

  // Resume checkpoint state
  const [currentResumeCheckpoint, setCurrentResumeCheckpoint] = useState<{
    id: string;
    name: string;
    epoch?: number;
  } | null>(resumeCheckpoint ?? null);
  const [epochOffset, setEpochOffset] = useState<number>(propEpochOffset);

  // Form fields
  const [params, setParams] = useState<DiffusionHyperparametersValues>({
    baseModel: initialPreset?.baseModel ?? "sdxl",
    triggerWord: initialPreset?.triggerWord ?? "",
    epochs: initialPreset?.epochs ?? 10,
    batchSize: initialPreset?.batchSize ?? 1,
    learningRate: initialPreset?.learningRate ?? "0.0001",
    rank: initialPreset?.rank ?? 16,
    alpha: initialPreset?.alpha ?? 16,
  });

  // Configurações avançadas de treino
  const [resolution, setResolution] = useState<number>(initialPreset?.resolution ?? 1024);
  const [gradientAccumulationSteps, setGradientAccumulationSteps] = useState<number>(
    initialPreset?.gradientAccumulationSteps ?? 1
  );
  const [optimizer, setOptimizer] = useState<"adamw8bit" | "adamw" | "prodigy">(
    initialPreset?.optimizer ?? "adamw8bit"
  );
  const [lrScheduler, setLrScheduler] = useState<"cosine" | "linear" | "constant" | "constant_with_warmup">(
    initialPreset?.lrScheduler ?? "cosine"
  );
  const [lrWarmupSteps, setLrWarmupSteps] = useState<number>(initialPreset?.lrWarmupSteps ?? 0);
  const [mixedPrecision, setMixedPrecision] = useState<"fp16" | "bf16" | "no">(
    initialPreset?.mixedPrecision ?? "fp16"
  );
  const [quantization, setQuantization] = useState<"none" | "4bit" | "8bit">(
    initialPreset?.quantization ?? "4bit"
  );
  const [checkpointInterval, setCheckpointInterval] = useState<number>(
    initialPreset?.checkpointInterval ?? 1
  );
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Amostras de validação (samples por época)
  const [enableSamples, setEnableSamples] = useState(initialPreset?.enableSamples ?? true);
  const [samplePrompt, setSamplePrompt] = useState(initialPreset?.samplePrompt ?? "");
  const [sampleInterval, setSampleInterval] = useState(initialPreset?.sampleInterval ?? 1);
  const [sampleSeed, setSampleSeed] = useState(initialPreset?.sampleSeed ? String(initialPreset.sampleSeed) : "42");

  const [outputName, setOutputName] = useState("");
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  useEffect(() => {
    if (resumeCheckpoint) {
      setCurrentResumeCheckpoint(resumeCheckpoint);
      setSelectedWeightId(resumeCheckpoint.id);
    }
  }, [resumeCheckpoint]);

  useEffect(() => {
    if (propEpochOffset != null) {
      setEpochOffset(propEpochOffset);
    }
  }, [propEpochOffset]);

  // Telemetria de hardware
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function loadTelem() {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      try {
        const t = await getTelemetry();
        if (!cancelled) setTelemetry(t);
      } catch {
        // Best-effort
      }
    }
    loadTelem();
    const timer = setInterval(loadTelem, 10000);
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        void loadTelem();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  // Estimativa preditiva de VRAM em GB
  const estimatedVram = useMemo(
    () =>
      estimateDiffusionVramGb(
        params.baseModel,
        params.batchSize,
        params.rank,
        resolution,
        optimizer,
        mixedPrecision,
        quantization,
      ),
    [params.baseModel, params.batchSize, params.rank, resolution, optimizer, mixedPrecision, quantization],
  );

  const nodeVramTotalGb = useMemo(() => {
    if (telemetry?.vramTotal && telemetry.vramTotal > 0) {
      return telemetry.vramTotal > 1000
        ? Math.round((telemetry.vramTotal / (1024 * 1024 * 1024)) * 10) / 10
        : telemetry.vramTotal;
    }
    return null;
  }, [telemetry?.vramTotal]);

  const oomRisk = useMemo<"safe" | "warning" | "danger">(() => {
    if (nodeVramTotalGb != null) {
      if (estimatedVram > nodeVramTotalGb) return "danger";
      if (estimatedVram > nodeVramTotalGb * 0.85) return "warning";
      return "safe";
    }
    if (estimatedVram >= 16) return "danger";
    if (estimatedVram >= 12) return "warning";
    return "safe";
  }, [estimatedVram, nodeVramTotalGb]);

  const deviceLabel = useMemo(() => {
    if (telemetry?.gpus && telemetry.gpus.length > 0) {
      return `${telemetry.gpus[0]} (${nodeVramTotalGb || 24} GB)`;
    }
    return "Host CPU (Modo Mock)";
  }, [telemetry?.gpus, nodeVramTotalGb]);

  function applyPreset(preset: Partial<DiffusionPreset> & { name: string }) {
    if (preset.baseModel) setParams((p) => ({ ...p, baseModel: preset.baseModel! }));
    if (preset.triggerWord !== undefined) setParams((p) => ({ ...p, triggerWord: preset.triggerWord! }));
    if (preset.epochs !== undefined) setParams((p) => ({ ...p, epochs: preset.epochs! }));
    if (preset.batchSize !== undefined) setParams((p) => ({ ...p, batchSize: preset.batchSize! }));
    if (preset.learningRate !== undefined) setParams((p) => ({ ...p, learningRate: String(preset.learningRate!) }));
    if (preset.rank !== undefined) {
      setParams((p) => ({ ...p, rank: preset.rank!, alpha: preset.alpha ?? preset.rank! }));
    }
    if (preset.resolution !== undefined) setResolution(preset.resolution);
    if (preset.gradientAccumulationSteps !== undefined) {
      setGradientAccumulationSteps(preset.gradientAccumulationSteps);
    }
    if (preset.optimizer !== undefined) setOptimizer(preset.optimizer);
    if (preset.lrScheduler !== undefined) setLrScheduler(preset.lrScheduler);
    if (preset.lrWarmupSteps !== undefined) setLrWarmupSteps(preset.lrWarmupSteps);
    if (preset.mixedPrecision !== undefined) setMixedPrecision(preset.mixedPrecision);
    if (preset.quantization !== undefined) setQuantization(preset.quantization);
    if (preset.checkpointInterval !== undefined) setCheckpointInterval(preset.checkpointInterval);
    if (preset.epochOffset !== undefined) setEpochOffset(preset.epochOffset);
    if (preset.enableSamples !== undefined) setEnableSamples(preset.enableSamples);
    if (preset.samplePrompt !== undefined) setSamplePrompt(preset.samplePrompt);
    if (preset.sampleInterval !== undefined) setSampleInterval(preset.sampleInterval);
    if (preset.sampleSeed !== undefined) setSampleSeed(String(preset.sampleSeed));

    showToast(`Preset aplicado: "${preset.name}"`, "info");
  }

  function handleExportPreset() {
    const presetData: DiffusionPreset = {
      name: `Preset LoRA ${params.baseModel.toUpperCase()} (${new Date().toLocaleDateString()})`,
      description: "Configurações exportadas do Hephaestus Studio",
      version: "1.0.0",
      baseModel: params.baseModel,
      triggerWord: params.triggerWord,
      epochs: params.epochs,
      batchSize: params.batchSize,
      learningRate: params.learningRate,
      rank: params.rank,
      alpha: params.alpha,
      resolution,
      gradientAccumulationSteps,
      optimizer,
      lrScheduler,
      lrWarmupSteps,
      mixedPrecision,
      quantization,
      checkpointInterval,
      epochOffset: epochOffset > 0 ? epochOffset : undefined,
      enableSamples,
      samplePrompt,
      sampleInterval,
      sampleSeed,
    };

    const blob = new Blob([JSON.stringify(presetData, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `hephaestus-preset-${params.baseModel}-${params.rank}rank.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast("Preset exportado com sucesso!", "success");
  }

  function handleImportPreset(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (event) => {
      try {
        const text = event.target?.result as string;
        const parsed = JSON.parse(text) as Record<string, any>;

        if (!parsed.baseModel && !parsed.epochs && !parsed.rank && !parsed.lora && !parsed.model) {
          showToast("Arquivo JSON não é um preset válido do Hephaestus.", "error");
          return;
        }

        const lora = parsed.lora || {};
        const samples = parsed.samples || {};
        const model = parsed.model || {};

        applyPreset({
          name: parsed.name || file.name.replace(".json", ""),
          baseModel: (parsed.baseModel || model.base || parsed.base_model || params.baseModel) as DiffusionBaseModel,
          triggerWord: parsed.triggerWord ?? parsed.trigger_word ?? params.triggerWord,
          epochs: typeof lora.epochs === "number" ? lora.epochs : typeof parsed.epochs === "number" ? parsed.epochs : params.epochs,
          batchSize:
            typeof lora.batch_size === "number"
              ? lora.batch_size
              : typeof parsed.batchSize === "number"
                ? parsed.batchSize
                : typeof parsed.batch_size === "number"
                  ? parsed.batch_size
                  : params.batchSize,
          learningRate:
            lora.learning_rate != null
              ? String(lora.learning_rate)
              : typeof parsed.learningRate === "string"
                ? parsed.learningRate
                : typeof parsed.learningRate === "number"
                  ? String(parsed.learningRate)
                  : parsed.learning_rate != null
                    ? String(parsed.learning_rate)
                    : params.learningRate,
          rank: typeof lora.rank === "number" ? lora.rank : typeof parsed.rank === "number" ? parsed.rank : params.rank,
          alpha: typeof lora.alpha === "number" ? lora.alpha : typeof parsed.alpha === "number" ? parsed.alpha : params.alpha,
          resolution:
            typeof lora.resolution === "number"
              ? lora.resolution
              : typeof parsed.resolution === "number"
                ? parsed.resolution
                : resolution,
          gradientAccumulationSteps:
            typeof lora.gradient_accumulation_steps === "number"
              ? lora.gradient_accumulation_steps
              : typeof parsed.gradientAccumulationSteps === "number"
                ? parsed.gradientAccumulationSteps
                : typeof parsed.gradient_accumulation_steps === "number"
                  ? parsed.gradient_accumulation_steps
                  : gradientAccumulationSteps,
          optimizer: lora.optimizer || parsed.optimizer || optimizer,
          lrScheduler: lora.lr_scheduler || parsed.lrScheduler || parsed.lr_scheduler || lrScheduler,
          lrWarmupSteps:
            typeof lora.lr_warmup_steps === "number"
              ? lora.lr_warmup_steps
              : typeof parsed.lrWarmupSteps === "number"
                ? parsed.lrWarmupSteps
                : typeof parsed.lr_warmup_steps === "number"
                  ? parsed.lr_warmup_steps
                  : lrWarmupSteps,
          mixedPrecision: lora.mixed_precision || parsed.mixedPrecision || parsed.mixed_precision || mixedPrecision,
          quantization: lora.quantization || parsed.quantization || quantization,
          checkpointInterval:
            typeof lora.checkpoint_interval === "number"
              ? lora.checkpoint_interval
              : typeof parsed.checkpointInterval === "number"
                ? parsed.checkpointInterval
                : typeof parsed.checkpoint_interval === "number"
                  ? parsed.checkpoint_interval
                  : checkpointInterval,
          epochOffset:
            typeof lora.epoch_offset === "number"
              ? lora.epoch_offset
              : typeof parsed.epochOffset === "number"
                ? parsed.epochOffset
                : typeof parsed.epoch_offset === "number"
                  ? parsed.epoch_offset
                  : epochOffset,
          enableSamples:
            samples.prompt != null
              ? Boolean(samples.prompt)
              : typeof parsed.enableSamples === "boolean"
                ? parsed.enableSamples
                : enableSamples,
          samplePrompt: samples.prompt ?? parsed.samplePrompt ?? parsed.sample_prompt ?? samplePrompt,
          sampleInterval:
            typeof samples.interval === "number"
              ? samples.interval
              : typeof parsed.sampleInterval === "number"
                ? parsed.sampleInterval
                : typeof parsed.sample_interval === "number"
                  ? parsed.sample_interval
                  : sampleInterval,
          sampleSeed:
            samples.seed != null
              ? String(samples.seed)
              : parsed.sampleSeed != null
                ? String(parsed.sampleSeed)
                : parsed.sample_seed != null
                  ? String(parsed.sample_seed)
                  : sampleSeed,
        });
      } catch {
        showToast("Erro ao processar o arquivo JSON de preset.", "error");
      } finally {
        if (fileInputRef.current) {
          fileInputRef.current.value = "";
        }
      }
    };
    reader.readAsText(file);
  }

  function handleAutoFixSafeParams() {
    setParams((prev) => ({
      ...prev,
      baseModel: "sd15",
      batchSize: 1,
      rank: 16,
      alpha: 16,
    }));
    setResolution(512);
    setGradientAccumulationSteps(2);
    setOptimizer("adamw8bit");
    setMixedPrecision("fp16");
    setQuantization("4bit");
    showToast(
      "Hiperparâmetros ajustados para o perfil leve de VRAM (SD 1.5, 512px, Batch 1, GA 2x, 8-bit AdamW, 4-bit Quant).",
      "info",
    );
  }

  // Load datasets
  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const all = await listDatasets();
        if (!cancelled) {
          // Datasets com imagens são elegíveis para treino de difusão
          setDatasets(all.filter((d) => d.imagesCount > 0));
        }
      } catch {
        // Best-effort
      } finally {
        if (!cancelled) setDatasetsLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, []);

  // Pre-select first eligible dataset
  useEffect(() => {
    if (selectedDatasetId) return;
    const firstEligible = datasets.find((d) => canTrainDiffusion(d));
    if (firstEligible) {
      setSelectedDatasetId(firstEligible.id);
    }
  }, [datasets, selectedDatasetId]);

  // Load existing diffusion models for weights selector
  useEffect(() => {
    let cancelled = false;
    async function loadModels() {
      try {
        const all = await listModels();
        if (!cancelled) {
          setDiffusionModels(all.items.filter((m) => m.engine === "diffusion"));
        }
      } catch {
        // Best-effort
      }
    }
    loadModels();
    return () => {
      cancelled = true;
    };
  }, []);

  const hasEligibleDataset = useMemo(
    () => datasets.some((d) => canTrainDiffusion(d)),
    [datasets],
  );

  const selectedDataset = useMemo(
    () => datasets.find((d) => d.id === selectedDatasetId),
    [datasets, selectedDatasetId],
  );

  const defaultSuggestedOutputName = useMemo(() => {
    const dsSlug = selectedDataset?.slug || "dataset";
    const trigger = params.triggerWord.trim()
      ? params.triggerWord.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-")
      : "lora";
    return `${dsSlug}-${params.baseModel}-${trigger}.safetensors`;
  }, [selectedDataset, params.baseModel, params.triggerWord]);

  const datasetOptions = useMemo<SelectOption<string>[]>(() => {
    return datasets.map((d) => {
      const ready = canTrainDiffusion(d);
      const reason = ready ? null : trainDiffusionDisabledReason(d);
      return {
        value: d.id,
        label: d.title,
        badge: (
          <span className="flex items-center gap-1.5">
            <span className="rounded-full border border-zinc-700/60 bg-zinc-800/60 px-1.5 py-0.5 font-mono text-[10px] text-zinc-300">
              {d.imagesCount} imgs
            </span>
            {d.category === "difusao" && (
              <span className="rounded-full border border-brand-500/30 bg-brand-500/10 px-1.5 py-0.5 font-mono text-[10px] text-brand-400">
                Difusão
              </span>
            )}
          </span>
        ),
        disabled: !ready,
        disabledReason: reason ?? undefined,
        icon: <IconDatabase className="w-3.5 h-3.5 text-brand-400" />,
      };
    });
  }, [datasets]);

  const weightOptions = useMemo<SelectOption<string>[]>(() => {
    return diffusionModels.map((m) => ({
      value: m.id,
      label: `${m.name} · ${formatBytes(m.bytes)}`,
      badge: (
        <span className="rounded-full border border-brand-500/30 bg-brand-500/10 px-1.5 py-0.5 font-mono text-[10px] text-brand-400">
          LoRA
        </span>
      ),
    }));
  }, [diffusionModels]);

  const batchOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 1, label: "1 (Mínima VRAM)" },
      { value: 2, label: "2" },
      { value: 4, label: "4" },
      { value: 8, label: "8 (Alta VRAM)" },
    ],
    [],
  );

  const rankOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 4, label: "4 (Ultra leve)" },
      { value: 8, label: "8 (Leve)" },
      { value: 16, label: "16 (Recomendado)" },
      { value: 32, label: "32 (Alta capacidade)" },
      { value: 64, label: "64 (Muito detalhado)" },
      { value: 128, label: "128 (Máximo detalhe)" },
    ],
    [],
  );

  const resolutionOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 512, label: "512 x 512 (Padrão SD 1.5 / Menor VRAM)" },
      { value: 768, label: "768 x 768 (Intermediário)" },
      { value: 1024, label: "1024 x 1024 (Padrão SDXL / FLUX)" },
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

  const optimizerOptions = useMemo<SelectOption<"adamw8bit" | "adamw" | "prodigy">[]>(
    () => [
      { value: "adamw8bit", label: "AdamW 8-bit (BitsAndBytes - Recomendado)" },
      { value: "adamw", label: "AdamW FP32 (Padrão PyTorch)" },
      { value: "prodigy", label: "Prodigy (Taxa adaptativa D-Adaptation)" },
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

  const quantizationOptions = useMemo<SelectOption<"none" | "4bit" | "8bit">[]>(
    () => [
      { value: "4bit", label: "4-bit NF4 (BitsAndBytes — Recomendado p/ GPUs ≤ 12GB)" },
      { value: "8bit", label: "8-bit BitsAndBytes (Equilíbrio p/ GPUs ≥ 16GB)" },
      { value: "none", label: "Nenhum (FP16/BF16 Pleno — GPUs ≥ 24GB)" },
    ],
    [],
  );

  const checkpointIntervalOptions = useMemo<SelectOption<number>[]>(
    () => [
      { value: 1, label: "1 época (Padrão — Checkpoint a cada época)" },
      { value: 2, label: "2 épocas" },
      { value: 5, label: "5 épocas" },
      { value: 10, label: "10 épocas" },
    ],
    [],
  );

  const parsedLr = parseFloat(params.learningRate);
  const epochsValid =
    Number.isInteger(params.epochs) &&
    params.epochs >= DIFFUSION_EPOCHS_MIN &&
    params.epochs <= DIFFUSION_EPOCHS_MAX;
  const lrValid = !isNaN(parsedLr) && parsedLr >= 1e-6 && parsedLr <= 0.01;
  const canSubmit = hasEligibleDataset && selectedDatasetId && epochsValid && lrValid && !busy;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

    if (!selectedDatasetId) {
      setTopError("Selecione um dataset.");
      return;
    }
    if (!epochsValid) {
      setTopError(`Épocas devem ser entre ${DIFFUSION_EPOCHS_MIN} e ${DIFFUSION_EPOCHS_MAX}.`);
      return;
    }
    if (!lrValid) {
      setTopError("Learning Rate deve estar entre 0.000001 e 0.01.");
      return;
    }

    setBusy(true);
    try {
      const result = await startDiffusionJob({
        datasetId: selectedDatasetId,
        baseModel: params.baseModel,
        triggerWord: params.triggerWord.trim() || undefined,
        epochs: params.epochs,
        batchSize: params.batchSize,
        learningRate: parsedLr,
        rank: params.rank,
        alpha: params.alpha,
        weights: selectedWeightId || null,
        orchestratorId: selectedOrchestratorId || null,
        outputName: outputName.trim() || undefined,
        samplePrompt: enableSamples && samplePrompt.trim() ? samplePrompt.trim() : undefined,
        sampleInterval: enableSamples ? sampleInterval : undefined,
        sampleSeed: enableSamples && sampleSeed.trim() ? parseInt(sampleSeed, 10) : undefined,
        resolution,
        gradientAccumulationSteps,
        optimizer,
        lrScheduler,
        lrWarmupSteps,
        mixedPrecision,
        quantization,
        checkpointInterval,
        epochOffset: epochOffset > 0 ? epochOffset : undefined,
      });

      showToast(
        `Job de difusão LoRA criado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );

      // Reset form
      setSelectedWeightId("");
      setSelectedOrchestratorId(null);
      setCurrentResumeCheckpoint(null);
      setEpochOffset(0);
      setCheckpointInterval(1);
      setOutputName("");
      setSamplePrompt("");
      setParams({
        baseModel: "sdxl",
        triggerWord: "",
        epochs: 10,
        batchSize: 1,
        learningRate: "0.0001",
        rank: 16,
        alpha: 16,
      });
      setResolution(1024);
      setGradientAccumulationSteps(1);
      setOptimizer("adamw8bit");
      setLrScheduler("cosine");
      setLrWarmupSteps(0);
      setMixedPrecision("fp16");
      setQuantization("4bit");
      onJobCreated?.(result.jobId);
    } catch (err) {
      if (err instanceof ApiError) {
        if (selectedWeightId && err.code === "not_found") {
          setTopError(
            "Modelo de pesos não encontrado — remova a seleção de pesos iniciais e tente novamente.",
          );
        } else {
          setTopError(diffusionErrorMessage(err.code));
        }
        return;
      }
      setTopError("Falha ao criar job de treino de difusão.");
    } finally {
      setBusy(false);
    }
  }

  // Empty state: no datasets with images
  if (!datasetsLoading && !hasEligibleDataset) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-2xl p-10 text-center border border-white/5 bg-zinc-900/40 backdrop-blur-sm">
        <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
          <IconDatabase className="h-6 w-6 text-zinc-600" />
        </span>
        <p className="text-sm font-medium text-zinc-200">
          Nenhum dataset com imagens encontrado
        </p>
        <p className="text-xs text-zinc-400 max-w-sm">
          Crie ou faça upload de imagens em um dataset para forjar um adaptador LoRA.
        </p>
        <Link
          href="/datasets"
          className={getButtonClasses({ variant: "primary", size: "md" })}
        >
          <IconDatabase className="h-3.5 w-3.5" />
          Ir para Datasets
        </Link>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-6 text-xs">
      {/* Cabeçalho do Card */}
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400">
          <IconSparkles className="h-4 w-4" />
        </div>
        <div>
          <h2 className="font-display text-sm font-bold text-white tracking-wide">
            Forja Difusão LoRA
          </h2>
          <p className="font-mono text-[11px] text-zinc-400">
            Ajuste fino de adaptadores para geração de imagem
          </p>
        </div>
      </div>

      {/* Badge Modo Continuação */}
      {currentResumeCheckpoint && (
        <div className="flex items-center justify-between gap-3 rounded-xl border border-sky-500/20 bg-sky-500/[0.06] px-3.5 py-2.5 backdrop-blur-sm">
          <div className="flex items-center gap-2 min-w-0">
            <IconSparkles className="size-3.5 text-sky-400 shrink-0" />
            <span className="text-xs text-zinc-300 truncate">
              <span className="font-semibold text-white">Modo Continuação:</span> Retomando de{" "}
              <span className="font-mono text-sky-300 font-medium">{currentResumeCheckpoint.name}</span>
              {epochOffset > 0 && (
                <span className="text-zinc-400 font-mono text-[11px] ml-1.5">
                  (+{epochOffset} épocas anteriores)
                </span>
              )}
            </span>
          </div>
          <button
            type="button"
            onClick={() => {
              setCurrentResumeCheckpoint(null);
              setSelectedWeightId("");
              setEpochOffset(0);
            }}
            className="text-zinc-400 hover:text-zinc-200 transition-colors p-1 rounded hover:bg-white/5 shrink-0 cursor-pointer"
            title="Cancelar modo de continuação"
          >
            <IconX className="size-3.5" />
          </button>
        </div>
      )}

      {topError && (
        <p
          role="alert"
          className="rounded-lg border border-rose-500/30 bg-rose-500/10 backdrop-blur-sm px-3 py-2 text-xs text-rose-300"
        >
          {topError}
        </p>
      )}

      {/* Barra de Presets & Importar/Exportar */}
      <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3 backdrop-blur-sm">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <IconSparkles className="size-3.5 text-brand-400" />
            <span className="font-display text-xs font-semibold text-zinc-200">
              Presets de Treinamento
            </span>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="file"
              ref={fileInputRef}
              onChange={handleImportPreset}
              accept=".json,application/json"
              className="hidden"
            />
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => fileInputRef.current?.click()}
              disabled={busy}
              leftIcon={<IconUpload className="size-3" />}
              className="font-mono text-[11px]"
            >
              Importar JSON
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={handleExportPreset}
              disabled={busy}
              leftIcon={<IconDownload className="size-3" />}
              className="font-mono text-[11px]"
            >
              Exportar JSON
            </Button>
          </div>
        </div>

        {/* Botões de presets rápidos */}
        <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              applyPreset({
                name: "FLUX.2 Klein 4B (4-bit NF4)",
                baseModel: "flux",
                epochs: 10,
                batchSize: 1,
                rank: 16,
                alpha: 16,
                learningRate: "0.00003",
                resolution: 1024,
                gradientAccumulationSteps: 1,
                optimizer: "adamw8bit",
                lrScheduler: "cosine",
                lrWarmupSteps: 0,
                mixedPrecision: "bf16",
                quantization: "4bit",
              })
            }
            className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40"
          >
            <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
              FLUX.2 Klein 4B
            </span>
            <span className="font-mono text-[10px] text-zinc-400 mt-0.5">
              1024px · 4-bit · ~10GB
            </span>
          </button>

          <button
            type="button"
            disabled={busy}
            onClick={() =>
              applyPreset({
                name: "Equilibrado (SDXL 1024)",
                baseModel: "sdxl",
                epochs: 10,
                batchSize: 1,
                rank: 16,
                alpha: 16,
                learningRate: "0.0001",
                resolution: 1024,
                gradientAccumulationSteps: 1,
                optimizer: "adamw8bit",
                lrScheduler: "cosine",
                lrWarmupSteps: 0,
                mixedPrecision: "fp16",
                quantization: "4bit",
              })
            }
            className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40"
          >
            <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
              SDXL Padrão
            </span>
            <span className="font-mono text-[10px] text-zinc-400 mt-0.5">
              1024px · Rank 16 · 8-bit
            </span>
          </button>

          <button
            type="button"
            disabled={busy}
            onClick={() =>
              applyPreset({
                name: "Eco VRAM (SD 1.5 512)",
                baseModel: "sd15",
                epochs: 10,
                batchSize: 1,
                rank: 8,
                alpha: 8,
                learningRate: "0.0001",
                resolution: 512,
                gradientAccumulationSteps: 2,
                optimizer: "adamw8bit",
                lrScheduler: "cosine",
                lrWarmupSteps: 0,
                mixedPrecision: "fp16",
                quantization: "4bit",
              })
            }
            className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40"
          >
            <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
              Eco 8 GB (SD 1.5)
            </span>
            <span className="font-mono text-[10px] text-zinc-400 mt-0.5">
              512px · Rank 8 · GA 2x
            </span>
          </button>

          <button
            type="button"
            disabled={busy}
            onClick={() =>
              applyPreset({
                name: "Alta Fidelidade (Rank 32)",
                baseModel: "sdxl",
                epochs: 15,
                batchSize: 1,
                rank: 32,
                alpha: 32,
                learningRate: "0.00005",
                resolution: 1024,
                gradientAccumulationSteps: 2,
                optimizer: "adamw8bit",
                lrScheduler: "cosine",
                lrWarmupSteps: 50,
                mixedPrecision: "fp16",
                quantization: "4bit",
              })
            }
            className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40"
          >
            <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
              Alta Fidelidade
            </span>
            <span className="font-mono text-[10px] text-zinc-400 mt-0.5">
              1024px · Rank 32 · GA 2x
            </span>
          </button>

          <button
            type="button"
            disabled={busy}
            onClick={() =>
              applyPreset({
                name: "Prodigy Adaptativo",
                baseModel: "sdxl",
                epochs: 10,
                batchSize: 1,
                rank: 16,
                alpha: 16,
                learningRate: "1.0",
                resolution: 1024,
                gradientAccumulationSteps: 2,
                optimizer: "prodigy",
                lrScheduler: "cosine",
                lrWarmupSteps: 50,
                mixedPrecision: "fp16",
                quantization: "4bit",
              })
            }
            className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40"
          >
            <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
              Auto LR Prodigy
            </span>
            <span className="font-mono text-[10px] text-zinc-400 mt-0.5">
              Adaptativo · LR 1.0 auto
            </span>
          </button>
        </div>
      </div>

      {/* Dataset selector */}
      <Select
        id="setup-diffusion-dataset"
        ref={firstRef}
        label="Dataset de Treinamento"
        options={datasetOptions}
        value={selectedDatasetId}
        onChange={(val) => setSelectedDatasetId(val)}
        placeholder="Selecione um dataset com imagens…"
        loading={datasetsLoading}
        loadingText="Carregando datasets…"
        emptyText="Nenhum dataset com imagens encontrado"
        disabled={busy}
        searchable={datasets.length > 5}
        fontMono
      />

      {/* Seletor de Modelo Base (Cards Selecionáveis) */}
      <div className="space-y-2">
        <label className="block text-xs font-medium text-zinc-300">
          Modelo Base
        </label>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          {/* SDXL 1.0 */}
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              setParams((p) => ({
                ...p,
                baseModel: "sdxl",
                learningRate: p.learningRate === "0.00003" ? "0.0001" : p.learningRate,
              }))
            }
            className={`flex flex-col text-left p-3.5 rounded-xl border transition-colors ${
              params.baseModel === "sdxl"
                ? "border-brand-500/50 bg-brand-500/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] ring-1 ring-brand-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                SDXL 1.0
              </span>
              <span
                className={`rounded-full px-2 py-0.5 font-mono text-[10px] ${
                  params.baseModel === "sdxl"
                    ? "border border-brand-500/30 bg-brand-500/20 text-brand-300"
                    : "border border-white/10 bg-white/5 text-zinc-400"
                }`}
              >
                ~12 GB VRAM
              </span>
            </div>
            <p className="text-[11px] text-zinc-400 leading-snug">
              Equilíbrio ideal entre fidelidade, estilos artísticos e fotorealismo.
            </p>
          </button>

          {/* FLUX.2 Klein 4B */}
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              setParams((p) => ({
                ...p,
                baseModel: "flux",
                learningRate: p.learningRate === "0.0001" ? "0.00003" : p.learningRate,
              }));
              setMixedPrecision("bf16");
            }}
            className={`flex flex-col text-left p-3.5 rounded-xl border transition-colors ${
              params.baseModel === "flux"
                ? "border-brand-500/50 bg-brand-500/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] ring-1 ring-brand-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                FLUX.2 Klein 4B
              </span>
              <span
                className={`rounded-full px-2 py-0.5 font-mono text-[10px] ${
                  params.baseModel === "flux"
                    ? "border border-brand-500/30 bg-brand-500/20 text-brand-300"
                    : "border border-white/10 bg-white/5 text-zinc-400"
                }`}
              >
                ~10 GB VRAM
              </span>
            </div>
            <p className="text-[11px] text-zinc-400 leading-snug">
              Modelo leve de 4B parâmetros com Flow Matching, ideal para LoRA rápido em GPUs de 10–12 GB.
            </p>
          </button>

          {/* Stable Diffusion 1.5 */}
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              setParams((p) => ({
                ...p,
                baseModel: "sd15",
                learningRate: p.learningRate === "0.00003" ? "0.0001" : p.learningRate,
              }))
            }
            className={`flex flex-col text-left p-3.5 rounded-xl border transition-colors ${
              params.baseModel === "sd15"
                ? "border-brand-500/50 bg-brand-500/[0.08] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.06)] ring-1 ring-brand-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                SD 1.5
              </span>
              <span
                className={`rounded-full px-2 py-0.5 font-mono text-[10px] ${
                  params.baseModel === "sd15"
                    ? "border border-brand-500/30 bg-brand-500/20 text-brand-300"
                    : "border border-white/10 bg-white/5 text-zinc-400"
                }`}
              >
                ~8 GB VRAM
              </span>
            </div>
            <p className="text-[11px] text-zinc-400 leading-snug">
              Mais leve, veloz e compatível com GPUs menores ou ambientes restritos.
            </p>
          </button>
        </div>
      </div>

      {/* Trigger Word */}
      <div className="space-y-1.5">
        <label htmlFor="trigger-word" className="block text-xs font-medium text-zinc-300">
          Trigger Word (Palavra-Gatilho)
        </label>
        <Input
          id="trigger-word"
          type="text"
          placeholder="ex: ohwx person, sks style, vintage anime"
          value={params.triggerWord}
          onChange={(e) => setParams((p) => ({ ...p, triggerWord: e.target.value }))}
          disabled={busy}
          className="font-mono text-xs"
        />
        <p className="text-[11px] font-mono text-zinc-500">
          Opcional. Prefixa automaticamente as legendas de cada imagem durante o empacotamento.
        </p>
      </div>

      {/* Nome do Adaptador / Modelo (outputName — ADR-0022 D1/D4) */}
      <div className="space-y-1.5">
        <label htmlFor="diffusion-output-name" className="block text-xs font-medium text-zinc-300">
          Nome do Adaptador / Modelo <span className="text-zinc-500 font-normal">(opcional)</span>
        </label>
        <Input
          id="diffusion-output-name"
          type="text"
          placeholder={defaultSuggestedOutputName}
          value={outputName}
          onChange={(e) => setOutputName(e.target.value)}
          disabled={busy}
          className="font-mono text-xs"
        />
        <p className="text-[11px] font-mono text-zinc-500">
          Nome personalizado para o arquivo .safetensors. Se omitido, o estúdio gerará um nome semântico inteligente.
        </p>
      </div>

      {/* Hiperparâmetros LoRA */}
      <div className="rounded-xl border border-white/10 bg-white/[0.01] p-4 space-y-4">
        <div className="flex items-center gap-2 pb-1 border-b border-white/5">
          <IconSettings className="size-3.5 text-brand-400" />
          <span className="font-display text-xs font-semibold text-zinc-200">
            Hiperparâmetros LoRA
          </span>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 items-start">
          {/* Épocas */}
          <Input
            id="diffusion-epochs"
            label={`Épocas (${DIFFUSION_EPOCHS_MIN}–${DIFFUSION_EPOCHS_MAX})`}
            type="number"
            min={DIFFUSION_EPOCHS_MIN}
            max={DIFFUSION_EPOCHS_MAX}
            value={params.epochs}
            onChange={(e) =>
              setParams((p) => ({ ...p, epochs: parseInt(e.target.value, 10) || 1 }))
            }
            disabled={busy}
            fontMono
            className="h-[38px] text-xs font-mono"
          />

          {/* Batch Size */}
          <Select
            id="diffusion-batch"
            label="Batch Size"
            options={batchOptions}
            value={params.batchSize}
            onChange={(val) => setParams((p) => ({ ...p, batchSize: Number(val) }))}
            disabled={busy}
            fontMono
            size="default"
          />

          {/* LoRA Rank */}
          <Select
            id="diffusion-rank"
            label="LoRA Rank"
            options={rankOptions}
            value={params.rank}
            onChange={(val) => {
              const num = Number(val);
              setParams((p) => ({ ...p, rank: num, alpha: num }));
            }}
            disabled={busy}
            fontMono
            size="default"
          />

          {/* Learning Rate */}
          <Input
            id="diffusion-lr"
            label="Learning Rate"
            type="text"
            value={params.learningRate}
            onChange={(e) => setParams((p) => ({ ...p, learningRate: e.target.value }))}
            disabled={busy}
            fontMono
            className="h-[38px] text-xs font-mono"
          />
        </div>
      </div>

      {/* Configurações Avançadas (Colapsável) */}
      <div className="rounded-xl border border-white/10 bg-white/[0.01] transition-colors">
        <button
          type="button"
          aria-expanded={showAdvanced}
          aria-controls="advanced-diffusion-settings"
          onClick={() => setShowAdvanced((prev) => !prev)}
          className="w-full flex items-center justify-between p-3.5 text-left hover:bg-white/[0.02] transition-colors focus:outline-none focus:ring-2 focus:ring-brand-500/40"
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
          <div className="flex flex-wrap items-center gap-1.5 font-mono text-[10px] text-zinc-400">
            <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
              {resolution}x{resolution}
            </span>
            <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
              GA: {gradientAccumulationSteps}x
            </span>
            <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
              {optimizer === "adamw8bit" ? "8-bit AdamW" : optimizer === "prodigy" ? "Prodigy" : "AdamW"}
            </span>
            <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
              {mixedPrecision.toUpperCase()}
            </span>
            <span className="rounded-md bg-white/[0.04] px-2 py-0.5 border border-white/10 text-zinc-300">
              {quantization === "4bit" ? "4-BIT NF4" : quantization === "8bit" ? "8-BIT BNB" : "FP16 PLENO"}
            </span>
          </div>
        </button>

        {showAdvanced && (
          <div id="advanced-diffusion-settings" className="p-4 pt-2 border-t border-white/5 space-y-4">
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              {/* Quantização do Modelo Base */}
              <Select
                id="diffusion-quant"
                label="Quantização do Modelo Base"
                hint={
                  params.baseModel === "flux"
                    ? "4-bit NF4 permite rodar em GPUs ≤ 12 GB. 8-bit exige ≥ 16 GB e Nenhum exige ≥ 24 GB."
                    : "Quantização BitsAndBytes do backbone para redução drástica de memória VRAM."
                }
                options={quantizationOptions}
                value={quantization}
                onChange={(val) => setQuantization(val as "none" | "4bit" | "8bit")}
                disabled={busy}
                fontMono
                size="default"
              />

              {/* Resolução de Treinamento */}
              <Select
                id="diffusion-res"
                label="Resolução de Entrada"
                hint="Imagens são ajustadas com recorte centrado e aspect ratio seguro."
                options={resolutionOptions}
                value={resolution}
                onChange={(val) => setResolution(Number(val))}
                disabled={busy}
                fontMono
                size="default"
              />

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
                    : "8-bit economiza ~2 GB de VRAM no estado do otimizador."
                }
                options={optimizerOptions}
                value={optimizer}
                onChange={(opt) => {
                  setOptimizer(opt);
                  if (opt === "prodigy" && params.learningRate === "0.0001") {
                    setParams((p) => ({ ...p, learningRate: "1.0" }));
                  } else if (opt !== "prodigy" && params.learningRate === "1.0") {
                    setParams((p) => ({ ...p, learningRate: "0.0001" }));
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

      {/* Weights selector (fine-tune previous LoRA) */}
      <Select
        id="setup-diffusion-weights"
        label="Pesos Anteriores (Fine-tune Incremental)"
        options={weightOptions}
        value={selectedWeightId}
        onChange={(val) => setSelectedWeightId(val)}
        placeholder="Do zero (criar novo adaptador LoRA)"
        disabled={busy}
        searchable={diffusionModels.length > 5}
        fontMono
      />

      {/* Nó de Execução (ADR-0015 D2) */}
      <NodeSelect
        value={selectedOrchestratorId}
        onChange={setSelectedOrchestratorId}
        disabled={busy}
        size="default"
      />

      {/* Amostras Visuais de Validação (Samples por Época) */}
      <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3 backdrop-blur-sm">
        <div className="flex items-center justify-between">
          <label
            htmlFor="enable-samples-toggle"
            className="flex items-center gap-2 cursor-pointer select-none"
          >
            <input
              id="enable-samples-toggle"
              type="checkbox"
              checked={enableSamples}
              onChange={(e) => setEnableSamples(e.target.checked)}
              disabled={busy}
              className="size-4 rounded border-white/20 bg-white/5 text-brand-500 focus:ring-brand-500/30"
            />
            <span className="font-mono text-xs font-semibold text-zinc-200 flex items-center gap-1.5">
              <IconImage className="size-3.5 text-brand-400" />
              Amostras Visuais de Validação
            </span>
          </label>
          <span className="font-mono text-[10px] text-zinc-400">
            {enableSamples ? "Ativado" : "Desativado"}
          </span>
        </div>

        {enableSamples && (
          <div className="space-y-3 pt-1 border-t border-white/5">
            <Input
              id="sample-prompt-input"
              label="Prompt de Teste para Amostras"
              hint="Uma imagem será sintetizada para você acompanhar a evolução visual no Action Center e página de jobs."
              type="text"
              value={samplePrompt}
              onChange={(e) => setSamplePrompt(e.target.value)}
              placeholder={
                params.triggerWord.trim()
                  ? `ex: a photo of ${params.triggerWord.trim()} subject in studio lighting`
                  : "ex: a photo of a cute robot in cinematic lighting, 8k"
              }
              disabled={busy}
              fontMono
              className="h-[38px] text-xs font-mono"
            />

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <Input
                id="sample-interval-input"
                label="Intervalo (a cada N épocas)"
                type="number"
                min={1}
                max={params.epochs}
                value={sampleInterval}
                onChange={(e) =>
                  setSampleInterval(Math.max(1, parseInt(e.target.value, 10) || 1))
                }
                disabled={busy}
                fontMono
                className="h-[38px] text-xs font-mono"
              />
              <Input
                id="sample-seed-input"
                label="Seed da Amostra (Fixa)"
                hint="Fixa o ruído para comparar a evolução sobre a mesma composição."
                type="number"
                min={0}
                value={sampleSeed}
                onChange={(e) => setSampleSeed(e.target.value)}
                placeholder="42"
                disabled={busy}
                fontMono
                className="h-[38px] text-xs font-mono"
              />
            </div>
          </div>
        )}
      </div>

      {/* Previsão de VRAM & Alertas Preventivos de CUDA OOM */}
      <div
        className={`rounded-xl border p-3.5 space-y-2.5 transition backdrop-blur-sm ${
          oomRisk === "danger"
            ? "border-rose-500/40 bg-rose-500/[0.06]"
            : oomRisk === "warning"
              ? "border-amber-500/35 bg-amber-500/[0.05]"
              : "border-white/10 bg-white/[0.02]"
        }`}
      >
        <div className="flex items-center justify-between font-mono text-[11px]">
          <span className="tracking-caps font-medium uppercase text-zinc-400 flex items-center gap-1.5">
            <IconZap className="size-3.5 text-brand-400" />
            VRAM Estimada para Treino
          </span>
          <span
            className={`font-semibold ${
              oomRisk === "danger"
                ? "text-rose-400"
                : oomRisk === "warning"
                  ? "text-amber-300"
                  : "text-zinc-100"
            }`}
          >
            ~{estimatedVram} GB {nodeVramTotalGb ? `/ ${nodeVramTotalGb} GB` : ""}
          </span>
        </div>

        {/* Barra de Consumo de VRAM */}
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800/80 border border-white/5">
          <div
            className={`h-full rounded-full transition-all duration-300 motion-reduce:transition-none ${
              oomRisk === "danger"
                ? "bg-rose-500"
                : oomRisk === "warning"
                  ? "bg-amber-400"
                  : "bg-brand-500"
            }`}
            style={{
              width: `${Math.min(
                100,
                Math.max(6, Math.round((estimatedVram / (nodeVramTotalGb || 24)) * 100)),
              )}%`,
            }}
          />
        </div>

        {/* Dispositivo de Destino */}
        <div className="flex items-center justify-between font-mono text-[11px] text-zinc-400">
          <span className="flex items-center gap-1">
            <IconCpu className="size-3" /> Dispositivo:
          </span>
          <span className="text-zinc-300 truncate max-w-[200px]" title={deviceLabel}>
            {deviceLabel}
          </span>
        </div>

        {/* Alerta Preventivo de CUDA OOM */}
        {oomRisk !== "safe" && (
          <div
            className={`rounded-lg border p-2.5 space-y-2 ${
              oomRisk === "danger"
                ? "border-rose-500/30 bg-rose-950/40 text-rose-200"
                : "border-amber-500/30 bg-amber-950/40 text-amber-200"
            }`}
          >
            <div className="flex items-start gap-2">
              <IconAlertTriangle
                className={`size-4 shrink-0 mt-0.5 ${
                  oomRisk === "danger" ? "text-rose-400" : "text-amber-400"
                }`}
              />
              <div className="space-y-1 font-mono text-[11px]">
                <p className="font-semibold text-white">
                  {oomRisk === "danger"
                    ? "Risco Crítico de Memória de Vídeo (CUDA OOM)"
                    : "Atenção: Uso de VRAM Elevado"}
                </p>
                <p className="text-zinc-300 leading-snug">
                  {oomRisk === "danger"
                    ? `A configuração selecionada requer ~${estimatedVram} GB de VRAM${
                        nodeVramTotalGb ? ` (capacidade do nó: ${nodeVramTotalGb} GB)` : ""
                      }. O job provavelmente falhará por falta de memória.`
                    : `A estimativa de ~${estimatedVram} GB opera próxima ao limite máximo de alocação da GPU.`}
                </p>
              </div>
            </div>

            {/* Ação de Auto-Fix */}
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={handleAutoFixSafeParams}
              className="w-full font-mono text-[11px]"
            >
              Ajustar para Perfil Leve (SD 1.5 · 512px · Batch 1 · GA 2x)
            </Button>
          </div>
        )}
      </div>

      {/* CTA de Iniciar Treino */}
      <div className="pt-2">
        <Button
          type="submit"
          variant="primary"
          size="lg"
          disabled={!canSubmit}
          loading={busy}
          leftIcon={<IconPlay className="size-3.5" />}
          className="w-full"
        >
          {busy ? "Iniciando Treino LoRA…" : "Iniciar Treino LoRA"}
        </Button>
      </div>
    </form>
  );
}
