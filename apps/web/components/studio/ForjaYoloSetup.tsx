"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import {
  IconPlay,
  IconDatabase,
  IconAlertTriangle,
  IconInfo,
  IconZap,
} from "@/components/icons";
import { Button, getButtonClasses } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
import { getTelemetry, startYoloJob } from "@/lib/jobs";
import { listDatasets, canTrainYolo, trainDisabledReason } from "@/lib/datasets";
import { listModels } from "@/lib/models";
import { formatBytes } from "@/lib/format";
import { ApiError } from "@/lib/api";
import { jobErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import type { Dataset, Model, Telemetry, YoloAugment } from "@/types/studio";
import {
  YoloHyperparameters,
  EPOCHS_MIN,
  EPOCHS_MAX,
  type YoloHyperparametersValues,
} from "./YoloHyperparameters";

/**
 * Estimativa preditiva de VRAM com base na arquitetura, tamanho do lote,
 * resolução e estados de momentos do otimizador selecionado.
 */
export function estimateYoloVramGb(
  model: string,
  batch: number,
  imgsz: number,
  optimizer: string,
): number {
  let baseWeightsGb = 1.2;
  let activationFactor = 1.0;

  if (model === "yolo11n") {
    baseWeightsGb = 0.8;
    activationFactor = 0.6;
  } else if (model === "yolo11m") {
    baseWeightsGb = 1.6;
    activationFactor = 1.0;
  } else if (model === "yolo11x") {
    baseWeightsGb = 3.2;
    activationFactor = 1.8;
  } else if (model === "yolov9-c") {
    baseWeightsGb = 2.2;
    activationFactor = 1.3;
  } else if (model === "yolo11-seg") {
    baseWeightsGb = 2.0;
    activationFactor = 1.5;
  }

  const resFactor = Math.pow(imgsz / 640, 2);
  const batchMemory = (batch / 16) * 1.8 * activationFactor * resFactor;

  let optOverhead = 0.4;
  if (optimizer === "AdamW") optOverhead = 0.8;
  if (optimizer === "Muon") optOverhead = 1.2;
  if (optimizer === "SGD") optOverhead = 0.3;

  return Math.round((baseWeightsGb + batchMemory + optOverhead) * 10) / 10;
}

interface Props {
  onJobCreated?: (jobId: string) => void;
  initialTelemetry?: Telemetry | null;
}

export default function ForjaYoloSetup({ onJobCreated }: Props) {
  const firstRef = useRef<SelectRefHandle>(null);

  // Datasets
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [datasetsLoading, setDatasetsLoading] = useState(true);
  const [selectedDatasetId, setSelectedDatasetId] = useState<string>("");

  // Models (for weights selector)
  const [yoloModels, setYoloModels] = useState<Model[]>([]);
  const [selectedWeightId, setSelectedWeightId] = useState<string>("");

  // Form fields — mirrors TrainYoloModal defaults
  const [params, setParams] = useState<YoloHyperparametersValues>({
    model: "yolo11m",
    epochs: 100,
    batch: 16,
    imgsz: 640,
    lr0: "0.01",
    optimizer: "AdamW",
    augment: {
      mosaic: true,
      mixupFlip: true,
    },
  });
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  // Telemetria de hardware do nó
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
    () => estimateYoloVramGb(params.model, params.batch, params.imgsz, params.optimizer),
    [params.model, params.batch, params.imgsz, params.optimizer],
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
      if (estimatedVram > nodeVramTotalGb * 0.8) return "warning";
      return "safe";
    }
    // Host CPU / Mock ou hardware sem VRAM exposta
    if (estimatedVram >= 16) return "danger";
    if (estimatedVram >= 10) return "warning";
    return "safe";
  }, [estimatedVram, nodeVramTotalGb]);

  const deviceLabel = useMemo(() => {
    if (telemetry?.gpus && telemetry.gpus.length > 0) {
      return `${telemetry.gpus[0]} (${nodeVramTotalGb || 24} GB)`;
    }
    return "Host CPU (Modo Mock)";
  }, [telemetry?.gpus, nodeVramTotalGb]);

  const handleParamChange = <K extends keyof YoloHyperparametersValues>(
    key: K,
    val: YoloHyperparametersValues[K],
  ) => {
    setParams((prev) => ({ ...prev, [key]: val }));
  };

  function handleAutoFixSafeParams() {
    setParams((prev) => ({
      ...prev,
      batch: 16,
      imgsz: 640,
      model: prev.model === "yolo11x" ? "yolo11m" : prev.model,
    }));
    showToast(
      "Hiperparâmetros ajustados para o perfil seguro de VRAM (Batch 16, ImgSz 640).",
      "info",
    );
  }

  // Load YOLO datasets
  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const all = await listDatasets();
        if (!cancelled) {
          setDatasets(all.filter((d) => d.category === "yolo"));
        }
      } catch {
        // Best-effort — lista fica vazia
      } finally {
        if (!cancelled) setDatasetsLoading(false);
      }
    }
    load();
    return () => { cancelled = true; };
  }, []);

  // Load YOLO models for weights selector
  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const res = await listModels();
        if (!cancelled) {
          setYoloModels(res.items.filter((m) => m.engine === "yolo"));
        }
      } catch {
        // Best-effort — dropdown mostra só "Do zero"
      }
    }
    load();
    return () => { cancelled = true; };
  }, []);

  // Auto-focus first field
  useEffect(() => {
    const t = setTimeout(() => firstRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, []);

  const eligibleDatasets = datasets.filter(canTrainYolo);
  const hasEligibleDataset = eligibleDatasets.length > 0;

  const datasetOptions = useMemo<SelectOption<string>[]>(() => {
    return datasets.map((d) => {
      const ready = canTrainYolo(d);
      const reason = ready ? null : trainDisabledReason(d);
      return {
        value: d.id,
        label: d.title,
        badge: (
          <span className="flex items-center gap-1.5 font-mono text-[11px] text-zinc-400">
            <span className="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-zinc-300">
              {d.imagesCount} imgs
            </span>
            <span className="text-zinc-500">·</span>
            <span className="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-zinc-300">
              {d.classes.length} cls
            </span>
          </span>
        ),
        disabled: !ready,
        disabledReason: reason ?? undefined,
        icon: <IconDatabase className="w-3.5 h-3.5 text-brand-400" />,
      };
    });
  }, [datasets]);

  const weightOptions = useMemo<SelectOption<string>[]>(() => {
    return yoloModels.map((m) => ({
      value: m.id,
      label: `${m.name} · ${formatBytes(m.bytes)}`,
      badge: m.source === "train" ? (
        <span className="rounded-full border border-brand-500/30 bg-brand-500/10 px-1.5 py-0.5 font-mono text-[10px] text-brand-400">
          Treino
        </span>
      ) : undefined,
    }));
  }, [yoloModels]);

  const parsedLr0 = parseFloat(params.lr0);
  const epochsValid = Number.isInteger(params.epochs) && params.epochs >= EPOCHS_MIN && params.epochs <= EPOCHS_MAX;
  const lr0Valid = !isNaN(parsedLr0) && parsedLr0 >= 1e-5 && parsedLr0 <= 0.1 + 1e-9;
  const canSubmit = hasEligibleDataset && selectedDatasetId && epochsValid && lr0Valid && !busy;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

    if (!selectedDatasetId) {
      setTopError("Selecione um dataset.");
      return;
    }
    if (!epochsValid) {
      setTopError(`Epochs deve ser entre ${EPOCHS_MIN} e ${EPOCHS_MAX}.`);
      return;
    }
    if (!lr0Valid) {
      setTopError("lr0 deve estar entre 0.00001 e 0.1.");
      return;
    }

    setBusy(true);
    try {
      const result = await startYoloJob({
        datasetId: selectedDatasetId,
        model: params.model,
        epochs: params.epochs,
        batch: params.batch,
        imgsz: params.imgsz,
        lr0: parsedLr0,
        optimizer: params.optimizer,
        augment: params.augment,
        weights: selectedWeightId || null,
      });
      showToast(
        `Job de treino criado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );
      // Reset form
      setSelectedDatasetId("");
      setSelectedWeightId("");
      setParams({
        model: "yolo11m",
        epochs: 100,
        batch: 16,
        imgsz: 640,
        lr0: "0.01",
        optimizer: "AdamW",
        augment: { mosaic: true, mixupFlip: true },
      });
      onJobCreated?.(result.jobId);
    } catch (err) {
      if (err instanceof ApiError) {
        // B1: este é submit de JOB — jobErrorMessage como fonte primária.
        if (selectedWeightId && err.code === "not_found") {
          setTopError(
            "Modelo de pesos não encontrado — remova a seleção de pesos iniciais e tente de novo.",
          );
        } else {
          setTopError(jobErrorMessage(err.code));
        }
        return;
      }
      setTopError("Falha ao criar job de treino.");
    } finally {
      setBusy(false);
    }
  }

  // ── Empty state: no eligible datasets ──
  if (!datasetsLoading && !hasEligibleDataset) {
    return (
      <div className="flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
        <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
          <IconDatabase className="h-6 w-6 text-zinc-700" />
        </span>
        <p className="text-sm font-medium text-zinc-200">
          Nenhum dataset YOLO elegível
        </p>
        <p className="text-xs text-zinc-400">
          Crie ou prepare um dataset YOLO com pelo menos 1 classe e 1 imagem para treinar.
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
    <form onSubmit={handleSubmit} className="space-y-5 text-xs">
      {/* Header */}
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400">
          <IconPlay className="h-4 w-4" />
        </div>
        <div>
          <h2 className="font-display text-sm font-bold text-white">
            Setup do Treino YOLO
          </h2>
          <p className="font-mono text-[11px] text-zinc-400">
            Configure e inicie um novo treinamento
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
        id="setup-dataset"
        ref={firstRef}
        label="Dataset"
        options={datasetOptions}
        value={selectedDatasetId}
        onChange={(val) => setSelectedDatasetId(val)}
        placeholder="Selecione um dataset…"
        loading={datasetsLoading}
        loadingText="Carregando datasets…"
        emptyText="Nenhum dataset YOLO elegível encontrado"
        disabled={busy}
        searchable={datasets.length > 5}
        fontMono
      />

      {/* Weights selector (fine-tune) */}
      <Select
        id="setup-weights"
        label="Pesos iniciais"
        options={weightOptions}
        value={selectedWeightId}
        onChange={(val) => setSelectedWeightId(val)}
        placeholder="Do zero (pré-treinado)"
        disabled={busy}
        searchable={yoloModels.length > 5}
        fontMono
      />

      {/* Shared YOLO Hyperparameters Form */}
      <YoloHyperparameters
        values={params}
        onChange={handleParamChange}
        disabled={busy}
      />

      {/* Previsão de VRAM & Alertas Preventivos de CUDA OOM */}
      <div
        className={`rounded-xl border p-3 space-y-2.5 transition backdrop-blur-sm ${
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
            VRAM Estimada
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
                  : "bg-brand-500"
            }`}
            style={{
              width: `${Math.min(
                100,
                Math.max(6, Math.round((estimatedVram / (nodeVramTotalGb || 16)) * 100)),
              )}%`,
            }}
          />
        </div>

        {/* Dispositivo de Destino */}
        <div className="flex items-center justify-between font-mono text-[11px] text-zinc-400">
          <span>Dispositivo:</span>
          <span className="text-zinc-300 truncate max-w-[180px]" title={deviceLabel}>
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
                    ? "Risco Crítico de CUDA OOM"
                    : "Alerta de VRAM Elevada"}
                </p>
                <p className="text-zinc-300 leading-snug">
                  {oomRisk === "danger"
                    ? `A combinação selecionada exige ~${estimatedVram} GB de VRAM${
                        nodeVramTotalGb ? ` (limite do nó: ${nodeVramTotalGb} GB)` : ""
                      }. O treinamento local falhará por falta de memória na GPU.`
                    : `A estimativa de ~${estimatedVram} GB opera próxima ao limite seguro de alocação da GPU.`}
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
              Ajustar para Perfil Seguro (Batch 16, ImgSz 640)
            </Button>
          </div>
        )}
      </div>

      {/* CTA */}
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
          {busy ? "Iniciando…" : "Iniciar Treino"}
        </Button>
      </div>
    </form>
  );
}
