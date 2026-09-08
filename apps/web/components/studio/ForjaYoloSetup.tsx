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
import { ApiError } from "@/lib/api";
import { Button } from "@/components/ui/Button";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
import { getTelemetry, startYoloJob } from "@/lib/jobs";
import { listDatasets } from "@/lib/datasets";
import { jobErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import type { Dataset, Telemetry, YoloAugment } from "@/types/studio";

const MODELS = ["yolo11n", "yolo11m", "yolo11x", "yolov9-c", "yolo11-seg"] as const;
const EPOCHS_MIN = 1;
const EPOCHS_MAX = 1000;
const BATCH_OPTIONS = [8, 16, 32, 64] as const;
const IMGSZ_OPTIONS = [416, 640, 1024] as const;
const OPTIMIZERS = ["AdamW", "SGD", "Muon"] as const;

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

  // Form fields — mirrors TrainYoloModal defaults
  const [model, setModel] = useState<string>("yolo11m");
  const [epochs, setEpochs] = useState<number>(100);
  const [batch, setBatch] = useState<number>(16);
  const [imgsz, setImgsz] = useState<number>(640);
  const [lr0, setLr0] = useState<string>("0.01");
  const [optimizer, setOptimizer] = useState<string>("AdamW");
  const [augment, setAugment] = useState<YoloAugment>({
    mosaic: true,
    mixupFlip: true,
  });
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  // Telemetria de hardware do nó
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function loadTelem() {
      try {
        const t = await getTelemetry();
        if (!cancelled) setTelemetry(t);
      } catch {
        // Best-effort
      }
    }
    loadTelem();
    const timer = setInterval(loadTelem, 10000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  // Estimativa preditiva de VRAM em GB
  const estimatedVram = useMemo(
    () => estimateYoloVramGb(model, batch, imgsz, optimizer),
    [model, batch, imgsz, optimizer],
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

  function handleAutoFixSafeParams() {
    setBatch(16);
    setImgsz(640);
    if (model === "yolo11x") setModel("yolo11m");
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

  // Auto-focus first field
  useEffect(() => {
    const t = setTimeout(() => firstRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, []);

  // Derive dataset eligibility (mirrors DatasetMenu.canTrain: category + classes + images)
  function datasetReady(d: Dataset): boolean {
    return d.category === "yolo" && d.classes.length >= 1 && d.imagesCount >= 1;
  }

  function datasetDisabledReason(d: Dataset): string | null {
    if (d.category !== "yolo") return "Treino disponível apenas para datasets YOLO.";
    if (d.classes.length < 1) return "Nenhuma classe definida";
    if (d.imagesCount < 1) return "Nenhuma imagem";
    return null;
  }

  const eligibleDatasets = datasets.filter(datasetReady);
  const hasEligibleDataset = eligibleDatasets.length > 0;

  const datasetOptions = useMemo<SelectOption<string>[]>(() => {
    return datasets.map((d) => {
      const ready = datasetReady(d);
      const reason = datasetDisabledReason(d);
      return {
        value: d.id,
        label: d.title,
        badge: (
          <span className="flex items-center gap-1.5 font-mono text-[11px] text-zinc-400">
            <span className="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-zinc-300">
              {d.imagesCount} imgs
            </span>
            <span className="text-zinc-600">·</span>
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

  const modelOptions = useMemo<SelectOption<string>[]>(() => {
    return MODELS.map((m) => {
      let badgeText: string | undefined;
      if (m === "yolo11n") badgeText = "Nano · Ultraleve";
      else if (m === "yolo11m") badgeText = "Médio · Padrão";
      else if (m === "yolo11x") badgeText = "Extra · Alta VRAM";
      else if (m === "yolo11-seg") badgeText = "Segmentação";
      return {
        value: m,
        label: m,
        badge: badgeText ? (
          <span className="font-mono text-[11px] text-zinc-400">
            {badgeText}
          </span>
        ) : undefined,
      };
    });
  }, []);

  const batchOptions = useMemo<SelectOption<number>[]>(() => {
    return BATCH_OPTIONS.map((b) => ({
      value: b,
      label: `${b}`,
    }));
  }, []);

  const imgszOptions = useMemo<SelectOption<number>[]>(() => {
    return IMGSZ_OPTIONS.map((s) => ({
      value: s,
      label: `${s}px`,
    }));
  }, []);

  const optimizerOptions = useMemo<SelectOption<string>[]>(() => {
    return OPTIMIZERS.map((o) => ({
      value: o,
      label: o,
    }));
  }, []);
  const parsedLr0 = parseFloat(lr0);
  const epochsValid = Number.isInteger(epochs) && epochs >= EPOCHS_MIN && epochs <= EPOCHS_MAX;
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
        model,
        epochs,
        batch,
        imgsz,
        lr0: parsedLr0,
        optimizer,
        augment,
      });
      showToast(
        `Job de treino criado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );
      // Reset form
      setSelectedDatasetId("");
      setModel("yolo11m");
      setEpochs(100);
      setBatch(16);
      setImgsz(640);
      setLr0("0.01");
      setOptimizer("AdamW");
      setAugment({ mosaic: true, mixupFlip: true });
      onJobCreated?.(result.jobId);
      openActionCenter();
    } catch (err) {
      if (err instanceof ApiError) {
        setTopError(jobErrorMessage(err.code));
        return;
      }
      setTopError("Falha ao criar job de treino.");
    } finally {
      setBusy(false);
    }
  }

  function toggleAugment(key: keyof YoloAugment) {
    setAugment((prev) => ({ ...prev, [key]: !prev[key] }));
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
        <p className="text-xs text-zinc-500">
          Crie ou prepare um dataset YOLO com pelo menos 1 classe e 1 imagem para treinar.
        </p>
        <Link
          href="/datasets"
          className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-4 text-sm font-medium whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4"
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
        <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
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
          className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
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

      {/* Modelo */}
      <Select
        id="setup-model"
        label="Modelo"
        options={modelOptions}
        value={model}
        onChange={(val) => setModel(val)}
        disabled={busy}
        fontMono
      />

      {/* Grid: Epochs / Batch / ImgSz */}
      <div className="grid grid-cols-3 gap-3">
        <div>
          <label
            htmlFor="setup-epochs"
            className="tracking-caps mb-1.5 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            Epochs
          </label>
          <input
            id="setup-epochs"
            type="number"
            min={EPOCHS_MIN}
            max={EPOCHS_MAX}
            value={epochs}
            onChange={(e) => setEpochs(Number(e.target.value))}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3.5 py-2 font-mono text-xs text-zinc-200 focus:border-brand-500 focus:outline-none"
          />
        </div>
        <div>
          <Select
            id="setup-batch"
            label="Batch"
            options={batchOptions}
            value={batch}
            onChange={(val) => setBatch(Number(val))}
            disabled={busy}
            fontMono
          />
        </div>
        <div>
          <Select
            id="setup-imgsz"
            label="ImgSz"
            options={imgszOptions}
            value={imgsz}
            onChange={(val) => setImgsz(Number(val))}
            disabled={busy}
            align="right"
            fontMono
          />
        </div>
      </div>

      {/* Grid: LR0 / Optimizer */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label
            htmlFor="setup-lr0"
            className="tracking-caps mb-1.5 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            LR0
          </label>
          <input
            id="setup-lr0"
            type="text"
            inputMode="decimal"
            value={lr0}
            onChange={(e) => setLr0(e.target.value)}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3.5 py-2 font-mono text-xs text-zinc-200 focus:border-brand-500 focus:outline-none"
          />
        </div>
        <div>
          <Select
            id="setup-optimizer"
            label="Otimizador"
            options={optimizerOptions}
            value={optimizer}
            onChange={(val) => setOptimizer(val)}
            disabled={busy}
            align="right"
            fontMono
          />
        </div>
      </div>

      {/* Augment toggles */}
      <div>
        <span className="tracking-caps mb-2 block font-mono text-[11px] font-medium uppercase text-zinc-300">
          Augmentação
        </span>
        <div className="flex gap-3">
          <button
            type="button"
            onClick={() => toggleAugment("mosaic")}
            disabled={busy}
            aria-pressed={augment.mosaic}
            className={`inline-flex h-9 items-center gap-2 rounded-lg border px-4 text-xs font-medium transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] disabled:pointer-events-none disabled:opacity-55 ${
              augment.mosaic
                ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
            }`}
          >
            Mosaic
          </button>
          <button
            type="button"
            onClick={() => toggleAugment("mixupFlip")}
            disabled={busy}
            aria-pressed={augment.mixupFlip}
            className={`inline-flex h-9 items-center gap-2 rounded-lg border px-4 text-xs font-medium transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] disabled:pointer-events-none disabled:opacity-55 ${
              augment.mixupFlip
                ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
            }`}
          >
            Mixup+Flip
          </button>
        </div>
      </div>

      {/* Previsão de VRAM & Alertas Preventivos de CUDA OOM */}
      <div
        className={`rounded-xl border p-3 space-y-2.5 transition ${
          oomRisk === "danger"
            ? "border-rose-500/40 bg-rose-500/[0.06]"
            : oomRisk === "warning"
              ? "border-amber-500/35 bg-amber-500/[0.05]"
              : "border-zinc-800 bg-black/40"
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
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-900 border border-zinc-800">
          <div
            className={`h-full rounded-full transition-all duration-300 ${
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
            <button
              type="button"
              onClick={handleAutoFixSafeParams}
              className="w-full inline-flex items-center justify-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.06] hover:bg-white/10 py-1 text-[11px] font-mono text-zinc-200 transition"
            >
              <span>Ajustar para Perfil Seguro (Batch 16, ImgSz 640)</span>
            </button>
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
          className="w-full"
        >
          {busy ? (
            "Iniciando…"
          ) : (
            <>
              <IconPlay className="h-3.5 w-3.5" />
              Iniciar Treino
            </>
          )}
        </Button>
      </div>
    </form>
  );
}
