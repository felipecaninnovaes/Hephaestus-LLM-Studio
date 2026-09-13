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
import type { Dataset, Model, Telemetry } from "@/types/studio";

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
 * Fator de Batch e Rank adicionam overhead progressivo.
 */
export function estimateDiffusionVramGb(
  baseModel: DiffusionBaseModel,
  batchSize: number,
  rank: number,
): number {
  let baseGb = 12.0;
  if (baseModel === "sd15") baseGb = 8.0;
  if (baseModel === "sdxl") baseGb = 12.0;
  if (baseModel === "flux") baseGb = 10.0;

  const batchMemory = (batchSize - 1) * (baseModel === "flux" ? 1.8 : baseModel === "sdxl" ? 2.0 : 1.2);
  const rankMemory = (rank / 64) * 0.8;

  return Math.round((baseGb + batchMemory + rankMemory) * 10) / 10;
}

interface Props {
  onJobCreated?: (jobId: string) => void;
}

export default function ForjaDifusaoSetup({ onJobCreated }: Props) {
  const firstRef = useRef<SelectRefHandle>(null);

  // Datasets
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [datasetsLoading, setDatasetsLoading] = useState(true);
  const [selectedDatasetId, setSelectedDatasetId] = useState<string>("");

  // Models (fine-tune previous LoRA weights)
  const [diffusionModels, setDiffusionModels] = useState<Model[]>([]);
  const [selectedWeightId, setSelectedWeightId] = useState<string>("");
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);

  // Form fields
  const [params, setParams] = useState<DiffusionHyperparametersValues>({
    baseModel: "sdxl",
    triggerWord: "",
    epochs: 10,
    batchSize: 1,
    learningRate: "0.0001",
    rank: 16,
    alpha: 16,
  });

  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

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
    () => estimateDiffusionVramGb(params.baseModel, params.batchSize, params.rank),
    [params.baseModel, params.batchSize, params.rank],
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

  function handleAutoFixSafeParams() {
    setParams((prev) => ({
      ...prev,
      baseModel: "sd15",
      batchSize: 1,
      rank: 16,
      alpha: 16,
    }));
    showToast(
      "Hiperparâmetros ajustados para o perfil leve de VRAM (SD 1.5, Batch 1, Rank 16).",
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
              <span className="rounded-full border border-sky-500/30 bg-sky-500/10 px-1.5 py-0.5 font-mono text-[10px] text-sky-400">
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
      });

      showToast(
        `Job de difusão LoRA criado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );

      // Reset form
      setSelectedWeightId("");
      setSelectedOrchestratorId(null);
      setParams({
        baseModel: "sdxl",
        triggerWord: "",
        epochs: 10,
        batchSize: 1,
        learningRate: "0.0001",
        rank: 16,
        alpha: 16,
      });
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
        <div className="flex h-9 w-9 items-center justify-center rounded-xl border border-sky-500/30 bg-sky-500/10 backdrop-blur-sm text-sky-400">
          <IconPlay className="h-4 w-4" />
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

      {topError && (
        <p
          role="alert"
          className="rounded-lg border border-rose-500/30 bg-rose-500/10 backdrop-blur-sm px-3 py-2 text-xs text-rose-300"
        >
          {topError}
        </p>
      )}

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
            onClick={() => setParams((p) => ({ ...p, baseModel: "sdxl" }))}
            className={`flex flex-col text-left p-3.5 rounded-xl border transition duration-150 ${
              params.baseModel === "sdxl"
                ? "border-sky-500/60 bg-sky-500/10 text-white shadow-sm ring-1 ring-sky-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                SDXL 1.0
              </span>
              <span className="rounded-full bg-sky-500/20 border border-sky-500/30 px-1.5 py-0.5 font-mono text-[9px] text-sky-300">
                ~12 GB VRAM
              </span>
            </div>
            <p className="font-mono text-[11px] text-zinc-400 leading-snug">
              Equilíbrio ideal entre fidelidade, estilos artísticos e fotorealismo.
            </p>
          </button>

          {/* FLUX.2 Klein 4B */}
          <button
            type="button"
            disabled={busy}
            onClick={() => setParams((p) => ({ ...p, baseModel: "flux" }))}
            className={`flex flex-col text-left p-3.5 rounded-xl border transition duration-150 ${
              params.baseModel === "flux"
                ? "border-sky-500/60 bg-sky-500/10 text-white shadow-sm ring-1 ring-sky-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                FLUX.2 Klein 4B
              </span>
              <span className="rounded-full bg-emerald-500/20 border border-emerald-500/30 px-1.5 py-0.5 font-mono text-[9px] text-emerald-300">
                ~10 GB VRAM
              </span>
            </div>
            <p className="font-mono text-[11px] text-zinc-400 leading-snug">
              Modelo leve de 4B parâmetros com Flow Matching, ideal para LoRA rápido em GPUs de 10–12 GB.
            </p>
          </button>

          {/* Stable Diffusion 1.5 */}
          <button
            type="button"
            disabled={busy}
            onClick={() => setParams((p) => ({ ...p, baseModel: "sd15" }))}
            className={`flex flex-col text-left p-3.5 rounded-xl border transition duration-150 ${
              params.baseModel === "sd15"
                ? "border-sky-500/60 bg-sky-500/10 text-white shadow-sm ring-1 ring-sky-500/30"
                : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
            }`}
          >
            <div className="flex items-center justify-between w-full mb-1.5">
              <span className="font-display font-semibold text-xs text-zinc-100">
                SD 1.5
              </span>
              <span className="rounded-full bg-emerald-500/20 border border-emerald-500/30 px-1.5 py-0.5 font-mono text-[9px] text-emerald-300">
                ~8 GB VRAM
              </span>
            </div>
            <p className="font-mono text-[11px] text-zinc-400 leading-snug">
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

      {/* Hiperparâmetros LoRA */}
      <div className="rounded-xl border border-white/10 bg-white/[0.01] p-4 space-y-4">
        <div className="flex items-center gap-2 pb-1 border-b border-white/5">
          <IconSettings className="size-3.5 text-sky-400" />
          <span className="font-display text-xs font-semibold text-zinc-200">
            Hiperparâmetros LoRA
          </span>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {/* Épocas */}
          <div className="space-y-1">
            <label htmlFor="diffusion-epochs" className="block text-[11px] font-mono text-zinc-400">
              Épocas ({DIFFUSION_EPOCHS_MIN}–{DIFFUSION_EPOCHS_MAX})
            </label>
            <Input
              id="diffusion-epochs"
              type="number"
              min={DIFFUSION_EPOCHS_MIN}
              max={DIFFUSION_EPOCHS_MAX}
              value={params.epochs}
              onChange={(e) =>
                setParams((p) => ({ ...p, epochs: parseInt(e.target.value, 10) || 1 }))
              }
              disabled={busy}
              className="font-mono text-xs"
            />
          </div>

          {/* Batch Size */}
          <div className="space-y-1">
            <label htmlFor="diffusion-batch" className="block text-[11px] font-mono text-zinc-400">
              Batch Size
            </label>
            <select
              id="diffusion-batch"
              value={params.batchSize}
              onChange={(e) =>
                setParams((p) => ({ ...p, batchSize: parseInt(e.target.value, 10) || 1 }))
              }
              disabled={busy}
              className="w-full rounded-lg border border-white/10 bg-zinc-900 px-3 py-2 text-xs font-mono text-zinc-200 focus:border-sky-500/50 focus:outline-none focus:ring-1 focus:ring-sky-500/50"
            >
              <option value={1}>1 (Mínima VRAM)</option>
              <option value={2}>2</option>
              <option value={4}>4</option>
              <option value={8}>8 (Alta VRAM)</option>
            </select>
          </div>

          {/* LoRA Rank */}
          <div className="space-y-1">
            <label htmlFor="diffusion-rank" className="block text-[11px] font-mono text-zinc-400">
              LoRA Rank (Dimensão)
            </label>
            <select
              id="diffusion-rank"
              value={params.rank}
              onChange={(e) => {
                const val = parseInt(e.target.value, 10) || 16;
                setParams((p) => ({ ...p, rank: val, alpha: val }));
              }}
              disabled={busy}
              className="w-full rounded-lg border border-white/10 bg-zinc-900 px-3 py-2 text-xs font-mono text-zinc-200 focus:border-sky-500/50 focus:outline-none focus:ring-1 focus:ring-sky-500/50"
            >
              <option value={4}>4 (Ultra leve)</option>
              <option value={8}>8 (Leve)</option>
              <option value={16}>16 (Recomendado)</option>
              <option value={32}>32 (Alta capacidade)</option>
              <option value={64}>64 (Muito detalhado)</option>
              <option value={128}>128 (Máximo detalhe)</option>
            </select>
          </div>

          {/* Learning Rate */}
          <div className="space-y-1">
            <label htmlFor="diffusion-lr" className="block text-[11px] font-mono text-zinc-400">
              Learning Rate
            </label>
            <Input
              id="diffusion-lr"
              type="text"
              value={params.learningRate}
              onChange={(e) => setParams((p) => ({ ...p, learningRate: e.target.value }))}
              disabled={busy}
              className="font-mono text-xs"
            />
          </div>
        </div>
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
            <IconZap className="size-3.5 text-sky-400" />
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
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-black/40 border border-white/10">
          <div
            className={`h-full rounded-full transition-all duration-300 motion-reduce:transition-none ${
              oomRisk === "danger"
                ? "bg-rose-500"
                : oomRisk === "warning"
                  ? "bg-amber-400"
                  : "bg-sky-500"
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
              Ajustar para Perfil Leve (SD 1.5, Batch 1, Rank 16)
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
