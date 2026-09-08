"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { IconPlay, IconDatabase } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { startYoloJob } from "@/lib/jobs";
import { listDatasets } from "@/lib/datasets";
import { jobErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import type { Dataset, YoloAugment } from "@/types/studio";

const MODELS = ["yolo11n", "yolo11m", "yolo11x", "yolov9-c", "yolo11-seg"] as const;
const EPOCHS_MIN = 1;
const EPOCHS_MAX = 1000;
const BATCH_OPTIONS = [8, 16, 32, 64] as const;
const IMGSZ_OPTIONS = [416, 640, 1024] as const;
const OPTIMIZERS = ["AdamW", "SGD", "Muon"] as const;

interface Props {
  onJobCreated?: (jobId: string) => void;
}

export default function ForjaYoloSetup({ onJobCreated }: Props) {
  const firstRef = useRef<HTMLSelectElement>(null);

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
          className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-4 text-xs font-medium whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4"
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
          <p className="font-mono text-[10px] text-zinc-400">
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
      <div>
        <label
          htmlFor="setup-dataset"
          className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
        >
          Dataset
        </label>
        {datasetsLoading ? (
          <p className="py-2 font-mono text-xs text-zinc-500">Carregando datasets…</p>
        ) : (
          <select
            id="setup-dataset"
            ref={firstRef}
            value={selectedDatasetId}
            onChange={(e) => setSelectedDatasetId(e.target.value)}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          >
            <option value="">Selecione um dataset…</option>
            {datasets.map((d) => {
              const ready = datasetReady(d);
              const reason = datasetDisabledReason(d);
              return (
                <option
                  key={d.id}
                  value={d.id}
                  disabled={!ready}
                  title={ready ? d.title : `${reason}`}
                >
                  {d.title} · {d.imagesCount} imagens · {d.classes.length} classes
                  {!ready ? ` — ${reason}` : ""}
                </option>
              );
            })}
          </select>
        )}
      </div>

      {/* Modelo */}
      <div>
        <label
          htmlFor="setup-model"
          className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
        >
          Modelo
        </label>
        <select
          id="setup-model"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          disabled={busy}
          className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
        >
          {MODELS.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </div>

      {/* Grid: Epochs / Batch / ImgSz */}
      <div className="grid grid-cols-3 gap-3">
        <div>
          <label
            htmlFor="setup-epochs"
            className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
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
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          />
        </div>
        <div>
          <label
            htmlFor="setup-batch"
            className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            Batch
          </label>
          <select
            id="setup-batch"
            value={batch}
            onChange={(e) => setBatch(Number(e.target.value))}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          >
            {BATCH_OPTIONS.map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label
            htmlFor="setup-imgsz"
            className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            ImgSz
          </label>
          <select
            id="setup-imgsz"
            value={imgsz}
            onChange={(e) => setImgsz(Number(e.target.value))}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          >
            {IMGSZ_OPTIONS.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Grid: LR0 / Optimizer */}
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label
            htmlFor="setup-lr0"
            className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
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
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          />
        </div>
        <div>
          <label
            htmlFor="setup-optimizer"
            className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            Otimizador
          </label>
          <select
            id="setup-optimizer"
            value={optimizer}
            onChange={(e) => setOptimizer(e.target.value)}
            disabled={busy}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none"
          >
            {OPTIMIZERS.map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>
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

      {/* CTA */}
      <div className="flex justify-end pt-2">
        <button
          type="submit"
          disabled={!canSubmit}
          className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
        >
          {busy ? (
            "Iniciando…"
          ) : (
            <>
              <IconPlay className="h-3.5 w-3.5" />
              Iniciar Treino
            </>
          )}
        </button>
      </div>
    </form>
  );
}
