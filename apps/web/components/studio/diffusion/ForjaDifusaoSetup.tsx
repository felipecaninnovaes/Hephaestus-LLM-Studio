"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  IconAlertTriangle,
  IconDatabase,
  IconPlay,
  IconSparkles,
  IconX,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
import { startDiffusionJob } from "@/lib/jobs";
import { useHardwareTelemetry } from "@/hooks/useHardwareTelemetry";
import { useVramEstimator } from "@/hooks/useVramEstimator";
import {
  canTrainDiffusion,
  listDatasets,
  trainDiffusionDisabledReason,
} from "@/lib/datasets";
import { listModels } from "@/lib/models";
import { formatBytes } from "@/lib/format";
import { ApiError } from "@/lib/api";
import { diffusionErrorMessage } from "@/types/studio";
import { showToast } from "@/components/ui/Toast";
import NodeSelect from "../NodeSelect";
import type {
  Dataset,
  DiffusionOptimizer,
  DiffusionPreset,
  Model,
} from "@/types/studio";
import {
  estimateDiffusionVramGb,
  type DiffusionBaseModel,
  type DiffusionHyperparametersValues,
} from "./estimateDiffusionVram";
import { DiffusionAdvancedSettings } from "./DiffusionAdvancedSettings";
import { DiffusionHyperparametersSection } from "./DiffusionHyperparametersSection";
import { DiffusionPresetBar } from "./DiffusionPresetBar";
import { DiffusionSamplesSection } from "./DiffusionSamplesSection";
import { DiffusionVramForecast } from "./DiffusionVramForecast";

export interface ForjaDifusaoSetupProps {
  onJobCreated?: (jobId: string) => void;
  /** Preset camelCase da Forja; `weights`/`outputName` vêm do rerun via paramsToPreset. */
  initialPreset?: Partial<DiffusionPreset> & {
    weights?: string | null;
    outputName?: string | null;
  };
  initialDatasetId?: string;
  resumeCheckpoint?: { id: string; name: string; epoch?: number } | null;
  epochOffset?: number;
}

export function ForjaDifusaoSetup({
  onJobCreated,
  initialPreset,
  initialDatasetId,
  resumeCheckpoint,
  epochOffset: propEpochOffset = 0,
}: ForjaDifusaoSetupProps) {
  const firstRef = useRef<SelectRefHandle>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Datasets
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [datasetsLoading, setDatasetsLoading] = useState(true);
  const [selectedDatasetId, setSelectedDatasetId] = useState<string>(
    initialDatasetId ?? "",
  );

  // Models (fine-tune previous LoRA weights)
  const [diffusionModels, setDiffusionModels] = useState<Model[]>([]);
  const [selectedWeightId, setSelectedWeightId] = useState<string>(
    resumeCheckpoint?.id ?? "",
  );
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<
    string | null
  >(null);
  /* Base custom (fatia pesos-custom-flux2): UUID de checkpoint kind=checkpoint; "" = preset oficial. */
  const [customModelId, setCustomModelId] = useState<string>("");
  /* Text encoder custom (kind=text_encoder). "" = encoder oficial BFL; só vale p/ arch flux-2. */
  const [textEncoderModelId, setTextEncoderModelId] = useState<string>("");

  // Resume checkpoint state
  const [currentResumeCheckpoint, setCurrentResumeCheckpoint] = useState<{
    id: string;
    name: string;
    epoch?: number;
  } | null>(resumeCheckpoint ?? null);
  const [epochOffset, setEpochOffset] = useState<number>(propEpochOffset);

  // Form parameters
  const [params, setParams] = useState<DiffusionHyperparametersValues>({
    baseModel: (initialPreset?.baseModel as DiffusionBaseModel) ?? "flux",
    triggerWord: initialPreset?.triggerWord ?? "",
    epochs: initialPreset?.epochs ?? 10,
    batchSize: initialPreset?.batchSize ?? 1,
    learningRate: String(initialPreset?.learningRate ?? "0.00003"),
    rank: initialPreset?.rank ?? 16,
    alpha: initialPreset?.alpha ?? 16,
  });
  // Nome do modelo/adaptador sugerido (outputName)
  const [outputName, setOutputName] = useState<string>(
    typeof initialPreset?.outputName === "string" ? initialPreset.outputName : "",
  );

  // Advanced options
  const [resolution, setResolution] = useState<number>(
    initialPreset?.resolution ?? 1024,
  );
  const [gradientAccumulationSteps, setGradientAccumulationSteps] =
    useState<number>(initialPreset?.gradientAccumulationSteps ?? 1);
  const [optimizer, setOptimizer] = useState<DiffusionOptimizer>(
    initialPreset?.optimizer ?? "paged_adamw8bit",
  );
  const [lrScheduler, setLrScheduler] = useState<
    "cosine" | "linear" | "constant" | "constant_with_warmup"
  >(initialPreset?.lrScheduler ?? "cosine");
  const [lrWarmupSteps, setLrWarmupSteps] = useState<number>(
    initialPreset?.lrWarmupSteps ?? 0,
  );
  const [mixedPrecision, setMixedPrecision] = useState<"fp16" | "bf16" | "no">(
    initialPreset?.mixedPrecision ?? "bf16",
  );
  const [quantization, setQuantization] = useState<
    "none" | "2bit" | "4bit" | "6bit" | "8bit"
  >(initialPreset?.quantization ?? "4bit");
  const [controlDatasetId, setControlDatasetId] = useState<string>(
    initialPreset?.controlDatasetId ?? "",
  );
  const [cacheTextEmbeddings, setCacheTextEmbeddings] = useState<boolean>(
    initialPreset?.cacheTextEmbeddings ?? false,
  );
  const [enableBucket, setEnableBucket] = useState<boolean>(
    initialPreset?.enableBucket ?? true,
  );
  const [checkpointInterval, setCheckpointInterval] = useState<number>(
    initialPreset?.checkpointInterval ?? 5,
  );

  // Visual validation samples
  const [enableSamples, setEnableSamples] = useState<boolean>(
    initialPreset?.enableSamples ?? false,
  );
  const [samplePrompt, setSamplePrompt] = useState<string>(
    initialPreset?.samplePrompt ?? "",
  );
  const [sampleInterval, setSampleInterval] = useState<number>(
    initialPreset?.sampleInterval ?? 1,
  );
  const [sampleSeed, setSampleSeed] = useState<string>(
    initialPreset?.sampleSeed ? String(initialPreset.sampleSeed) : "",
  );
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  // A página /difusao lê `hephaestus_diffusion_resume` e injeta o preset via
  // props — mas o componente monta antes dos valores chegarem. Esta flag
  // aplica o preset da primeira navegação (resume/rerun) uma única vez,
  // sem sobrescrever edições posteriores do usuário.
  const presetAppliedRef = useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: aplicação one-shot do preset de resume/rerun — applyPreset é recriada por render; incluí-la re-aplicaria o preset sobre edições do usuário
  useEffect(() => {
    if (presetAppliedRef.current) return;
    const hasResume =
      initialPreset !== undefined ||
      (initialDatasetId ?? "") !== "" ||
      resumeCheckpoint != null ||
      propEpochOffset > 0;
    if (!hasResume) return;
    presetAppliedRef.current = true;
    applyPreset({ name: "Resume", ...(initialPreset ?? {}) });
    if (initialDatasetId) setSelectedDatasetId(initialDatasetId);
    if (resumeCheckpoint) {
      setCurrentResumeCheckpoint(resumeCheckpoint);
      setSelectedWeightId(resumeCheckpoint.id);
    }
    if (propEpochOffset > 0) setEpochOffset(propEpochOffset);
  }, [initialPreset, initialDatasetId, resumeCheckpoint, propEpochOffset]);

  // Telemetria de hardware e VRAM do nó
  const { nodeVramTotalGb, deviceLabel } = useHardwareTelemetry();

  function applyPreset(
    preset: Partial<DiffusionPreset> & {
      name: string;
      weights?: string | null;
      outputName?: string | null;
    },
  ) {
    if (preset.baseModel) {
      setParams((p) => ({ ...p, baseModel: preset.baseModel as DiffusionBaseModel }));
      setCustomModelId("");
      if (preset.baseModel !== "flux") setTextEncoderModelId("");
    }
    if (preset.triggerWord !== undefined)
      setParams((p) => ({ ...p, triggerWord: preset.triggerWord ?? "" }));
    if (preset.epochs !== undefined)
      setParams((p) => ({ ...p, epochs: preset.epochs ?? 10 }));
    if (preset.batchSize !== undefined)
      setParams((p) => ({ ...p, batchSize: preset.batchSize ?? 1 }));
    if (preset.learningRate !== undefined)
      setParams((p) => ({ ...p, learningRate: String(preset.learningRate ?? "1e-4") }));
    if (preset.rank !== undefined) {
      setParams((p) => ({
        ...p,
        rank: preset.rank ?? 16,
        alpha: preset.alpha ?? preset.rank ?? 16,
      }));
    } else if (preset.alpha !== undefined) {
      setParams((p) => ({ ...p, alpha: preset.alpha ?? 16 }));
    }
    if (preset.resolution !== undefined) setResolution(preset.resolution);
    if (preset.gradientAccumulationSteps !== undefined) {
      setGradientAccumulationSteps(preset.gradientAccumulationSteps);
    }
    if (preset.optimizer !== undefined) setOptimizer(preset.optimizer);
    if (preset.lrScheduler !== undefined) setLrScheduler(preset.lrScheduler);
    if (preset.lrWarmupSteps !== undefined)
      setLrWarmupSteps(preset.lrWarmupSteps);
    if (preset.mixedPrecision !== undefined)
      setMixedPrecision(preset.mixedPrecision);
    if (preset.quantization !== undefined) setQuantization(preset.quantization);
    if (preset.controlDatasetId !== undefined)
      setControlDatasetId(preset.controlDatasetId ?? "");
    if (preset.cacheTextEmbeddings !== undefined)
      setCacheTextEmbeddings(preset.cacheTextEmbeddings);
    if (preset.enableBucket !== undefined) setEnableBucket(preset.enableBucket);
    if (preset.checkpointInterval !== undefined)
      setCheckpointInterval(preset.checkpointInterval);
    if (preset.epochOffset !== undefined) setEpochOffset(preset.epochOffset);
    if (preset.enableSamples !== undefined)
      setEnableSamples(preset.enableSamples);
    if (preset.samplePrompt !== undefined)
      setSamplePrompt(preset.samplePrompt ?? "");
    if (preset.sampleInterval !== undefined)
      setSampleInterval(preset.sampleInterval);
    if (preset.sampleSeed !== undefined)
      setSampleSeed(String(preset.sampleSeed));
    // Rerun via paramsToPreset: UUID de pesos (fine-tune) e nome do adaptador.
    if (preset.weights !== undefined)
      setSelectedWeightId(preset.weights ?? "");
    if (preset.outputName !== undefined)
      setOutputName(preset.outputName ?? "");

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
      controlDatasetId: controlDatasetId || undefined,
      cacheTextEmbeddings,
      enableBucket,
      checkpointInterval,
      enableSamples,
      samplePrompt,
      sampleInterval,
      sampleSeed,
    };

    const blob = new Blob([JSON.stringify(presetData, null, 2)], {
      type: "application/json",
    });
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
        const parsed = JSON.parse(text) as Record<string, unknown>;

        // Aceita presets camelCase da Forja E o training_config.json real da
        // engine (YAML convertido: snake_case, `lora:` aninhado, `model:` como
        // string de arch, `output_name`/`checkpoint_interval`/`epoch_offset` no
        // topo, `samples:` opcional).
        if (
          !parsed.baseModel &&
          !parsed.epochs &&
          !parsed.rank &&
          !parsed.lora &&
          !parsed.model &&
          !parsed.output_name &&
          !parsed.data
        ) {
          showToast(
            "Arquivo JSON não é um preset válido do Hephaestus.",
            "error",
          );
          return;
        }

        const lora = (parsed.lora as Record<string, unknown> | undefined) || {};
        const samples =
          (parsed.samples as Record<string, unknown> | undefined) || {};
        // Na config da engine, `model:` é string de arch ("sdxl",
        // "flux-2-klein-4b"→"flux", "sd15"); presets futuros podem usar objeto.
        // `output_name`/`checkpoint_interval`/`epoch_offset` vivem no topo;
        // `data:` percorrida como fallback aninhado (shape "sem lora/data").
        const modelNode =
          typeof parsed.model === "object" && parsed.model !== null
            ? (parsed.model as Record<string, unknown>)
            : {};
        const rawArch =
          typeof parsed.model === "string" ? parsed.model : modelNode.base;
        const engineBaseModel =
          typeof rawArch === "string"
            ? rawArch === "flux-2-klein-4b"
              ? "flux"
              : rawArch
            : undefined;
        const dataSection =
          typeof parsed.data === "object" && parsed.data !== null
            ? (parsed.data as Record<string, unknown>)
            : undefined;
        const engineOutputName =
          typeof parsed.output_name === "string" ? parsed.output_name : undefined;

        applyPreset({
          name:
            typeof parsed.name === "string"
              ? parsed.name
              : file.name.replace(".json", ""),
          baseModel: (parsed.baseModel ||
            engineBaseModel ||
            parsed.base_model ||
            params.baseModel) as DiffusionBaseModel,
          triggerWord:
            typeof lora.trigger_word === "string"
              ? lora.trigger_word
              : typeof parsed.triggerWord === "string"
                ? parsed.triggerWord
                : typeof parsed.trigger_word === "string"
                  ? parsed.trigger_word
                  : params.triggerWord,
          epochs:
            typeof lora.epochs === "number"
              ? lora.epochs
              : typeof parsed.epochs === "number"
                ? parsed.epochs
                : params.epochs,
          batchSize:
            typeof lora.batch_size === "number"
              ? lora.batch_size
              : typeof parsed.batchSize === "number"
                ? parsed.batchSize
                : typeof parsed.batch_size === "number"
                  ? parsed.batch_size
                  : typeof parsed.train_batch_size === "number"
                    ? parsed.train_batch_size
                    : typeof dataSection?.batch_size === "number"
                      ? dataSection.batch_size
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
          rank:
            typeof lora.rank === "number"
              ? lora.rank
              : typeof parsed.rank === "number"
                ? parsed.rank
                : params.rank,
          alpha:
            typeof lora.alpha === "number"
              ? lora.alpha
              : typeof parsed.alpha === "number"
                ? parsed.alpha
                : params.alpha,
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
          optimizer: (lora.optimizer ||
            parsed.optimizer ||
            optimizer) as DiffusionOptimizer,
          lrScheduler: (lora.lr_scheduler ||
            parsed.lrScheduler ||
            parsed.lr_scheduler ||
            lrScheduler) as
            | "cosine"
            | "linear"
            | "constant"
            | "constant_with_warmup",
          lrWarmupSteps:
            typeof lora.lr_warmup_steps === "number"
              ? lora.lr_warmup_steps
              : typeof parsed.lrWarmupSteps === "number"
                ? parsed.lrWarmupSteps
                : typeof parsed.lr_warmup_steps === "number"
                  ? parsed.lr_warmup_steps
                  : lrWarmupSteps,
          mixedPrecision: (lora.mixed_precision ||
            parsed.mixedPrecision ||
            parsed.mixed_precision ||
            mixedPrecision) as "fp16" | "bf16" | "no",
          quantization: (lora.quantization ||
            parsed.quantization ||
            quantization) as "none" | "2bit" | "4bit" | "6bit" | "8bit",
          controlDatasetId:
            lora.control_dataset_path != null
              ? undefined
              : typeof parsed.controlDatasetId === "string"
                ? parsed.controlDatasetId
                : typeof parsed.control_dataset_id === "string"
                  ? parsed.control_dataset_id
                  : controlDatasetId,
          cacheTextEmbeddings:
            typeof lora.cache_text_embeddings === "boolean"
              ? lora.cache_text_embeddings
              : typeof parsed.cacheTextEmbeddings === "boolean"
                ? parsed.cacheTextEmbeddings
                : cacheTextEmbeddings,
          enableBucket:
            typeof lora.enable_bucket === "boolean"
              ? lora.enable_bucket
              : typeof parsed.enableBucket === "boolean"
                ? parsed.enableBucket
                : enableBucket,
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
          samplePrompt:
            typeof samples.prompt === "string"
              ? samples.prompt
              : typeof parsed.samplePrompt === "string"
                ? parsed.samplePrompt
                : typeof parsed.sample_prompt === "string"
                  ? parsed.sample_prompt
                  : samplePrompt,
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
          // A config da engine não tem UUID de pesos — só o nome do artefato.
          // Nunca sobrescreve a seleção de fine-tune atual com vazio.
          ...(engineOutputName ? { outputName: engineOutputName } : {}),
        });
        if (typeof parsed.epoch_offset === "number") {
          showToast(
            `Config da engine importada (epoch_offset ${parsed.epoch_offset} aplicado).`,
            "info",
          );
        }
        if (parsed.custom_checkpoint_path != null) {
          showToast(
            "Config da engine referencia checkpoint custom de staging — modelo base mantido.",
            "info",
          );
        }
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
      rank: 8,
      alpha: 8,
    }));
    setResolution(512);
    setGradientAccumulationSteps(2);
    setOptimizer("paged_adamw8bit");
    setQuantization("4bit");
    setCustomModelId("");
    setTextEncoderModelId("");
    showToast(
      "Configuração ajustada para perfil econômico seguro (SD 1.5 · 512px · Batch 1 · GA 2x · QLoRA 4-bit).",
      "info",
    );
  }

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const all = await listDatasets();
        if (!cancelled) {
          setDatasets(all);
          const firstEligible = all.find((d) => canTrainDiffusion(d));
          if (firstEligible && !selectedDatasetId) {
            setSelectedDatasetId(firstEligible.id);
          }
        }
      } catch (err) {
        if (!cancelled) {
          if (err instanceof ApiError && err.status === 401) {
            setTopError("Sessão expirada. Faça login novamente.");
          } else {
            setTopError(
              "Não foi possível carregar os datasets disponíveis para treino de difusão.",
            );
          }
        }
      } finally {
        if (!cancelled) setDatasetsLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [selectedDatasetId]);

  useEffect(() => {
    let cancelled = false;
    async function loadModels() {
      try {
        const all = await listModels();
        if (!cancelled) {
          setDiffusionModels(
            all.items.filter(
              (m) =>
                m.engine === "diffusion" ||
                m.kind === "checkpoint" ||
                m.kind === "text_encoder",
            ),
          );
        }
      } catch {
        // Modelos são opcionais para fine-tuning
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
            <span className="rounded-full border border-zinc-700/60 bg-zinc-800/60 px-1.5 py-0.5 font-mono text-3xs text-zinc-300">
              {d.imagesCount} imgs
            </span>
            {d.category === "difusao" && (
              <span className="rounded-full border border-brand-500/30 bg-brand-500/10 px-1.5 py-0.5 font-mono text-3xs text-brand-400">
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
      label: m.name,
      description: `${m.engine} · ${m.source} · ${formatBytes(m.bytes)}`,
      badge: (
        <span className="rounded-full border border-zinc-700/60 bg-zinc-800/60 px-1.5 py-0.5 font-mono text-3xs text-zinc-300">
          {formatBytes(m.bytes)}
        </span>
      ),
    }));
  }, [diffusionModels]);

  const checkpointModels = useMemo(
    () =>
      diffusionModels.filter(
        (m) => m.engine === "diffusion" && m.kind === "checkpoint",
      ),
    [diffusionModels],
  );
  const textEncoderModels = useMemo(
    () =>
      diffusionModels.filter(
        (m) => m.engine === "diffusion" && m.kind === "text_encoder",
      ),
    [diffusionModels],
  );

  /* Arch efetivo do treino: custom ⇒ arch do checkpoint; senão o preset ("flux" = flux-2-klein-4b). */
  const trainEffectiveArch = useMemo(() => {
    if (customModelId) {
      return (
        checkpointModels.find((m) => m.id === customModelId)?.arch ?? null
      );
    }
    return params.baseModel === "flux" ? "flux-2-klein-4b" : params.baseModel;
  }, [customModelId, checkpointModels, params.baseModel]);
  const isTrainFlux2 = trainEffectiveArch === "flux-2-klein-4b";

  /* VRAM estimada pelo arch EFETIVO (custom flux-2 custa como flux-2-klein-4b). */
  const estimatedVram = useMemo(
    () =>
      estimateDiffusionVramGb(
        trainEffectiveArch === "flux-2-klein-4b"
          ? "flux"
          : trainEffectiveArch === "sdxl" || trainEffectiveArch === "sd15" || trainEffectiveArch === "qwen-image-2.1"
            ? trainEffectiveArch
            : params.baseModel,
        params.batchSize,
        params.rank,
        resolution,
        optimizer,
        mixedPrecision,
        quantization,
      ),
    [
      trainEffectiveArch,
      params.baseModel,
      params.batchSize,
      params.rank,
      resolution,
      optimizer,
      mixedPrecision,
      quantization,
    ],
  );

  const { oomRisk } = useVramEstimator(estimatedVram, nodeVramTotalGb);

  const trainBaseOptions = useMemo<SelectOption<string>[]>(() => {
    const opts: SelectOption<string>[] = [
      {
        value: "preset:sdxl",
        label: "SDXL 1.0 (oficial)",
        description: "Equilíbrio fidelidade/estilo",
      },
      {
        value: "preset:flux",
        label: "FLUX.2 Klein 4B (oficial)",
        description: "LoRA rápido em GPUs 10–12 GB",
      },
      {
        value: "preset:sd15",
        label: "SD 1.5 (oficial)",
        description: "Leve p/ GPUs menores",
      },
      {
        value: "preset:qwen-image-2.1",
        label: "Qwen-Image-2.1 (oficial)",
        description: "7B Single-Stream DiT · 1024/2048px",
      },
    ];
    const byArch: Record<string, Model[]> = {};
    for (const m of checkpointModels) {
      const key = m.arch ?? "sem arch detectado";
      if (!byArch[key]) byArch[key] = [];
      byArch[key].push(m);
    }
    for (const arch of Object.keys(byArch).sort()) {
      for (const m of byArch[arch]) {
        opts.push({
          value: m.id,
          label: `${m.name} (custom · ${arch})`,
          description: `checkpoint ${arch} · ${m.source}`,
        });
      }
    }
    if (
      customModelId &&
      !checkpointModels.some((m) => m.id === customModelId)
    ) {
      opts.push({
        value: customModelId,
        label: "Modelo removido — faça upload em Modelos & Pesos",
        description: "checkpoint indisponível",
      });
    }
    return opts;
  }, [checkpointModels, customModelId]);
  const trainBaseValue = customModelId
    ? customModelId
    : `preset:${params.baseModel}`;

  const trainEncoderOptions = useMemo<SelectOption<string>[]>(() => {
    const opts: SelectOption<string>[] = [
      {
        value: "",
        label: "Encoder oficial BFL (padrão)",
        description: "Qwen3 do repo BFL",
      },
    ];
    for (const m of textEncoderModels) {
      opts.push({
        value: m.id,
        label: m.name,
        description: `text_encoder${m.arch ? ` ${m.arch}` : ""} · ${m.source}`,
      });
    }
    if (
      textEncoderModelId &&
      !textEncoderModels.some((m) => m.id === textEncoderModelId)
    ) {
      opts.push({
        value: textEncoderModelId,
        label: "Encoder removido — faça upload em Modelos & Pesos",
        description: "text_encoder indisponível",
      });
    }
    return opts;
  }, [textEncoderModels, textEncoderModelId]);

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

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

    if (!selectedDatasetId) {
      setTopError("Selecione um dataset válido para treino de difusão.");
      firstRef.current?.focus();
      return;
    }

    const ds = datasets.find((d) => d.id === selectedDatasetId);
    if (ds && !canTrainDiffusion(ds)) {
      setTopError(
        trainDiffusionDisabledReason(ds) ??
          "Dataset inválido para treino de difusão.",
      );
      return;
    }

    const lr = parseFloat(params.learningRate);
    if (Number.isNaN(lr) || lr <= 0) {
      setTopError(
        "Learning Rate inválida. Use notação decimal ou científica (ex: 0.0001 ou 1e-4).",
      );
      return;
    }

    setBusy(true);

    try {
      const res = await startDiffusionJob({
        datasetId: selectedDatasetId,
        baseModel: customModelId ? undefined : params.baseModel,
        customModelId: customModelId || null,
        textEncoderModelId:
          isTrainFlux2 && textEncoderModelId ? textEncoderModelId : null,
        triggerWord: params.triggerWord.trim() || undefined,
        epochs: params.epochs,
        batchSize: params.batchSize,
        learningRate: lr,
        rank: params.rank,
        alpha: params.alpha,
        weights: selectedWeightId || null,
        orchestratorId: selectedOrchestratorId || null,
        samplePrompt:
          enableSamples && samplePrompt.trim()
            ? samplePrompt.trim()
            : undefined,
        sampleInterval: enableSamples ? sampleInterval : undefined,
        sampleSeed:
          enableSamples && sampleSeed.trim()
            ? parseInt(sampleSeed, 10)
            : undefined,
        resolution,
        gradientAccumulationSteps,
        optimizer,
        lrScheduler,
        lrWarmupSteps,
        mixedPrecision,
        quantization,
        controlDatasetId: controlDatasetId || null,
        cacheTextEmbeddings,
        enableBucket,
        checkpointInterval,
        epochOffset: epochOffset > 0 ? epochOffset : undefined,
        outputName: outputName.trim() ? outputName.trim() : undefined,
      });

      showToast("Job de treino de difusão criado com sucesso!", "success");
      onJobCreated?.(res.jobId);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.status === 401) {
          setTopError("Sessão expirada. Faça login novamente.");
        } else {
          setTopError(diffusionErrorMessage(err.code));
        }
      } else {
        setTopError("Erro inesperado ao iniciar o treino de difusão.");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-6 text-xs">
      {/* Cabeçalho do Card */}
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400">
          <IconSparkles className="h-4 w-4" />
        </div>
        <div>
          <h2 className="font-display text-sm font-semibold text-white">
            Forja de Difusão · Treinamento LoRA
          </h2>
          <p className="font-mono text-2xs text-zinc-400">
            Ajuste fino de modelos de difusão (FLUX.2 Klein 4B, SDXL e SD 1.5)
            com pesos Low-Rank Adaptation.
          </p>
        </div>
      </div>

      {/* Badge Modo Continuação */}
      {currentResumeCheckpoint && (
        <div className="flex items-center justify-between gap-3 rounded-xl border border-sky-500/20 bg-sky-500/[0.06] px-3.5 py-2.5 backdrop-blur-sm">
          <div className="flex items-center gap-2 min-w-0">
            <span className="flex size-2 shrink-0 rounded-full bg-sky-400 animate-pulse" />
            <div className="min-w-0 font-mono text-2xs">
              <span className="font-semibold text-sky-300">
                Modo Continuação (Retomada):
              </span>{" "}
              <span className="text-zinc-200 truncate">
                {currentResumeCheckpoint.name}
              </span>
              {epochOffset > 0 && (
                <span className="ml-1 text-sky-400">
                  (a partir da época {epochOffset})
                </span>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={() => {
              setCurrentResumeCheckpoint(null);
              setEpochOffset(0);
              setSelectedWeightId("");
              showToast("Modo continuação desativado", "info");
            }}
            className="flex items-center gap-1 rounded-md border border-white/10 bg-white/[0.03] px-2 py-1 font-mono text-3xs text-zinc-400 hover:border-white/20 hover:text-zinc-200 transition-colors shrink-0 cursor-pointer"
          >
            <IconX className="size-3" />
            Cancelar Retomada
          </button>
        </div>
      )}

      {/* Barra de Presets & Importar/Exportar */}
      <DiffusionPresetBar
        busy={busy}
        fileInputRef={fileInputRef}
        onApplyPreset={applyPreset}
        onExportPreset={handleExportPreset}
        onImportPreset={handleImportPreset}
      />

      {/* Dataset selector */}
      <Select
        id="setup-diffusion-dataset"
        ref={firstRef}
        label="Dataset de Treino"
        placeholder={
          datasetsLoading
            ? "Carregando datasets…"
            : hasEligibleDataset
              ? "Selecione o dataset para treino…"
              : "Nenhum dataset elegível"
        }
        options={datasetOptions}
        value={selectedDatasetId}
        onChange={setSelectedDatasetId}
        disabled={busy || datasetsLoading || !hasEligibleDataset}
      />

      {/* Dataset de controle (regularização) — opcional, nunca igual ao principal */}
      <Select
        id="setup-diffusion-control-dataset"
        label="Dataset de controle (regularização)"
        hint="Opcional. Dataset com imagens/captions de referência para preservar conceitos prévios do modelo e mitigar esquecimento catastrófico (regularização)."
        placeholder="Nenhum (treino padrão sem regularização)"
        options={[
          {
            value: "",
            label: "Nenhum (sem regularização)",
            description: "Treino regular sem dataset de controle",
          },
          ...datasetOptions.filter((opt) => opt.value !== selectedDatasetId),
        ]}
        value={controlDatasetId}
        onChange={setControlDatasetId}
        disabled={busy || datasetsLoading}
      />

      {/* Cache Text Embeddings — pré-computa embeddings das captions */}
      <div className="rounded-lg border border-white/10 bg-white/[0.02] p-3 space-y-1.5">
        <label
          htmlFor="cache-text-embeddings-toggle"
          className="flex items-start gap-2 cursor-pointer select-none"
        >
          <input
            id="cache-text-embeddings-toggle"
            type="checkbox"
            checked={cacheTextEmbeddings}
            onChange={(e) => setCacheTextEmbeddings(e.target.checked)}
            disabled={busy}
            className="mt-0.5 size-4 rounded border-white/20 bg-white/5 text-brand-500 focus:ring-brand-500/30"
          />
          <span className="font-mono text-xs font-semibold text-zinc-200 leading-tight">
            Pré-computar Embeddings de Texto (Cache Text Embeddings)
          </span>
        </label>
        <p className="text-3xs leading-relaxed text-zinc-400 pl-6">
          Calcula os embeddings das legendas uma única vez antes do treino.
          Acelera o tempo por época em 25–40% ao custo de não permitir
          modificações em tempo de execução nos prompts.
        </p>
      </div>

      {/* Seletor de Modelo Base (presets oficiais + checkpoints custom por arch) */}
      <div className="space-y-2">
        <span className="block text-xs font-medium text-zinc-300">
          Modelo Base
        </span>
        <Select
          id="setup-diffusion-base-model"
          options={trainBaseOptions}
          value={trainBaseValue}
          onChange={(val) => {
            if (val.startsWith("preset:")) {
              const base = val.replace("preset:", "") as DiffusionBaseModel;
              setCustomModelId("");
              if (base === "qwen-image-2.1") {
                setParams((p) => ({ ...p, baseModel: base, learningRate: "0.0002" }));
                setResolution(1024);
              } else {
                setParams((p) => ({ ...p, baseModel: base }));
              }
              if (base !== "flux") setTextEncoderModelId("");
            } else {
              setCustomModelId(val);
              const hit = checkpointModels.find((m) => m.id === val);
              if (hit?.arch) {
                const mapped =
                  hit.arch === "flux-2-klein-4b"
                    ? "flux"
                    : (hit.arch as DiffusionBaseModel);
                setParams((p) => ({ ...p, baseModel: mapped }));
                if (mapped !== "flux") setTextEncoderModelId("");
              }
            }
          }}
          disabled={busy}
        />
        <p className="text-2xs font-mono text-zinc-500">
          Escolha um preset oficial ou um checkpoint custom carregado na aba
          Modelos & Pesos.
        </p>
      </div>

      {/* Text encoder (somente flux-2-klein-4b) */}
      <Select
        id="setup-diffusion-text-encoder"
        label="Text encoder"
        hint="Somente arquitetura FLUX.2. Selecione o encoder padrão BFL ou um text_encoder custom da aba Modelos & Pesos."
        options={trainEncoderOptions}
        value={textEncoderModelId}
        onChange={setTextEncoderModelId}
        disabled={busy || !isTrainFlux2}
      />

      {/* Hiperparâmetros LoRA & Nome do Modelo */}
      <DiffusionHyperparametersSection
        params={params}
        setParams={setParams}
        outputName={outputName}
        setOutputName={setOutputName}
        defaultSuggestedOutputName={defaultSuggestedOutputName}
        busy={busy}
        batchOptions={batchOptions}
        rankOptions={rankOptions}
      />

      {/* Configurações Avançadas (Colapsável) */}
      <DiffusionAdvancedSettings
        baseModel={params.baseModel}
        learningRate={params.learningRate}
        onLearningRateChange={(lr) =>
          setParams((p) => ({ ...p, learningRate: lr }))
        }
        quantization={quantization}
        setQuantization={setQuantization}
        resolution={resolution}
        setResolution={setResolution}
        enableBucket={enableBucket}
        setEnableBucket={setEnableBucket}
        gradientAccumulationSteps={gradientAccumulationSteps}
        setGradientAccumulationSteps={setGradientAccumulationSteps}
        optimizer={optimizer}
        setOptimizer={setOptimizer}
        lrScheduler={lrScheduler}
        setLrScheduler={setLrScheduler}
        lrWarmupSteps={lrWarmupSteps}
        setLrWarmupSteps={setLrWarmupSteps}
        mixedPrecision={mixedPrecision}
        setMixedPrecision={setMixedPrecision}
        checkpointInterval={checkpointInterval}
        setCheckpointInterval={setCheckpointInterval}
        busy={busy}
      />

      {/* Weights selector (fine-tune previous LoRA) */}
      <Select
        id="setup-diffusion-weights"
        label="Pesos Anteriores (Fine-tune Incremental)"
        placeholder="Nenhum (treino do zero)"
        options={[
          {
            value: "",
            label: "Nenhum (treino do zero)",
            description: "Inicializa pesos LoRA aleatórios",
          },
          ...weightOptions,
        ]}
        value={selectedWeightId}
        onChange={setSelectedWeightId}
        disabled={busy}
      />

      {/* Nó de Execução (ADR-0015 D2) */}
      <NodeSelect
        value={selectedOrchestratorId}
        onChange={setSelectedOrchestratorId}
        disabled={busy}
      />

      {/* Amostras Visuais de Validação (Samples por Época) */}
      <DiffusionSamplesSection
        enableSamples={enableSamples}
        setEnableSamples={setEnableSamples}
        samplePrompt={samplePrompt}
        setSamplePrompt={setSamplePrompt}
        sampleInterval={sampleInterval}
        setSampleInterval={setSampleInterval}
        sampleSeed={sampleSeed}
        setSampleSeed={setSampleSeed}
        triggerWord={params.triggerWord}
        epochs={params.epochs}
        busy={busy}
      />

      {/* Previsão de VRAM & Alertas Preventivos de CUDA OOM */}
      <DiffusionVramForecast
        estimatedVram={estimatedVram}
        nodeVramTotalGb={nodeVramTotalGb}
        deviceLabel={deviceLabel}
        oomRisk={oomRisk}
        onAutoFixSafeParams={handleAutoFixSafeParams}
      />

      {/* Erro Top-Level */}
      {topError && (
        <div className="flex items-start gap-2.5 rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-rose-300 backdrop-blur-sm">
          <IconAlertTriangle className="size-4 shrink-0 mt-0.5 text-rose-400" />
          <div className="text-xs leading-relaxed">{topError}</div>
        </div>
      )}

      {/* CTA de Iniciar Treino */}
      <div className="pt-2">
        <Button
          type="submit"
          variant="primary"
          size="lg"
          disabled={busy || datasetsLoading || !hasEligibleDataset}
          loading={busy}
          leftIcon={<IconPlay className="size-4" />}
          className="w-full font-display text-sm tracking-wide shadow-lg shadow-brand-500/20"
        >
          {busy
            ? "Iniciando Treinamento LoRA…"
            : currentResumeCheckpoint
              ? "Continuar Treino do Checkpoint"
              : "Iniciar Treinamento LoRA"}
        </Button>
      </div>
    </form>
  );
}

export default ForjaDifusaoSetup;
