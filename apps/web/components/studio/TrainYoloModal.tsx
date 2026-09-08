"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconPlay, IconX } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { startYoloJob } from "@/lib/jobs";
import { jobErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import type { YoloAugment } from "@/types/studio";

const MODELS = ["yolo11n", "yolo11m", "yolo11x", "yolov9-c", "yolo11-seg"] as const;
const EPOCHS_MIN = 1;
const EPOCHS_MAX = 1000;
const BATCH_OPTIONS = [8, 16, 32, 64] as const;
const IMGSZ_OPTIONS = [416, 640, 1024] as const;
const OPTIMIZERS = ["AdamW", "SGD", "Muon"] as const;

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
}

export default function TrainYoloModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
}: Props) {
  const router = useRouter();
  const firstRef = useRef<HTMLSelectElement>(null);
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

  useEffect(() => {
    if (!open) return;
    setModel("yolo11m");
    setEpochs(100);
    setBatch(16);
    setImgsz(640);
    setLr0("0.01");
    setOptimizer("AdamW");
    setAugment({ mosaic: true, mixupFlip: true });
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => firstRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  const parsedLr0 = parseFloat(lr0);
  const epochsValid = Number.isInteger(epochs) && epochs >= EPOCHS_MIN && epochs <= EPOCHS_MAX;
  const lr0Valid = !isNaN(parsedLr0) && parsedLr0 >= 1e-5 && parsedLr0 <= 0.1 + 1e-9;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

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
        datasetId,
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
      onClose();
      onJobCreated();
      openActionCenter();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
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

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="train-yolo-title"
        className="glass-modal relative w-full max-w-lg rounded-2xl p-6 text-zinc-100 shadow-2xl max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-white/10 pb-4">
          <div className="flex items-center space-x-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
              <IconPlay className="h-4 w-4" />
            </div>
            <div>
              <h3
                id="train-yolo-title"
                className="font-display text-sm font-bold text-white"
              >
                Treinar YOLO
              </h3>
              <p className="truncate font-mono text-[10px] text-zinc-400" title={datasetTitle}>
                {datasetTitle}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar modal"
            className="rounded-lg border border-transparent bg-transparent p-1 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="mt-4 space-y-4 text-xs">
          {topError && (
            <p
              role="alert"
              className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {topError}
            </p>
          )}

          {/* Modelo */}
          <div>
            <label
              htmlFor="train-model"
              className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
            >
              Modelo
            </label>
            <select
              id="train-model"
              ref={firstRef}
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
                htmlFor="train-epochs"
                className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
              >
                Epochs
              </label>
              <input
                id="train-epochs"
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
                htmlFor="train-batch"
                className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
              >
                Batch
              </label>
              <select
                id="train-batch"
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
                htmlFor="train-imgsz"
                className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
              >
                ImgSz
              </label>
              <select
                id="train-imgsz"
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
                htmlFor="train-lr0"
                className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
              >
                LR0
              </label>
              <input
                id="train-lr0"
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
                htmlFor="train-optimizer"
                className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
              >
                Otimizador
              </label>
              <select
                id="train-optimizer"
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
          <div className="flex justify-end space-x-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              disabled={busy}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-4 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              Cancelar
            </button>
            <button
              type="submit"
              disabled={busy}
              className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              {busy ? "Iniciando…" : "Iniciar"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
