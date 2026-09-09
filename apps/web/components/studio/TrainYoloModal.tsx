"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconPlay } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
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
  const firstRef = useRef<SelectRefHandle>(null);
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
    <Modal
      open={open}
      onClose={onClose}
      title="Treinar YOLO"
      description={
        <span className="truncate font-mono text-[11px] text-zinc-400 block" title={datasetTitle}>
          {datasetTitle}
        </span>
      }
      icon={<IconPlay className="h-4 w-4" />}
      maxWidth="lg"
      busy={busy}
      ariaLabel="Treinar YOLO"
    >
      <form onSubmit={handleSubmit} className="space-y-4 text-xs">
          {topError && (
            <p
              role="alert"
              className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {topError}
            </p>
          )}

          {/* Modelo */}
          <Select
            id="train-model"
            ref={firstRef}
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
              <Input
                id="train-epochs"
                label="Epochs"
                type="number"
                min={EPOCHS_MIN}
                max={EPOCHS_MAX}
                value={epochs}
                onChange={(e) => setEpochs(Number(e.target.value))}
                disabled={busy}
                fontMono
              />
            </div>
            <div>
              <Select
                id="train-batch"
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
                id="train-imgsz"
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
              <Input
                id="train-lr0"
                label="LR0"
                type="text"
                inputMode="decimal"
                value={lr0}
                onChange={(e) => setLr0(e.target.value)}
                disabled={busy}
                fontMono
              />
            </div>
            <div>
              <Select
                id="train-optimizer"
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
              <Button
                type="button"
                variant={augment.mosaic ? "primary" : "secondary"}
                size="md"
                onClick={() => toggleAugment("mosaic")}
                disabled={busy}
                aria-pressed={augment.mosaic}
              >
                Mosaic
              </Button>
              <Button
                type="button"
                variant={augment.mixupFlip ? "primary" : "secondary"}
                size="md"
                onClick={() => toggleAugment("mixupFlip")}
                disabled={busy}
                aria-pressed={augment.mixupFlip}
              >
                Mixup+Flip
              </Button>
            </div>
          </div>

          {/* CTA */}
          <div className="flex justify-end space-x-2 pt-2">
            <Button
              type="button"
              variant="ghost"
              size="md"
              onClick={onClose}
              disabled={busy}
            >
              Cancelar
            </Button>
            <Button
              type="submit"
              variant="primary"
              size="lg"
              loading={busy}
            >
              Iniciar
            </Button>
          </div>
        </form>
    </Modal>
  );
}
