"use client";

import React, { useMemo } from "react";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption, type SelectRefHandle } from "@/components/ui/Select";
import type { YoloAugment } from "@/types/studio";

export const YOLO_MODELS = [
  "yolo11n",
  "yolo11m",
  "yolo11x",
  "yolov9-c",
  "yolo11-seg",
] as const;

export const EPOCHS_MIN = 1;
export const EPOCHS_MAX = 1000;
export const BATCH_OPTIONS = [8, 16, 32, 64] as const;
export const IMGSZ_OPTIONS = [416, 640, 1024] as const;
export const OPTIMIZERS = ["AdamW", "SGD", "Muon"] as const;

export interface YoloHyperparametersValues {
  model: string;
  epochs: number;
  batch: number;
  imgsz: number;
  lr0: string;
  optimizer: string;
  augment: YoloAugment;
}

export interface YoloHyperparametersProps {
  values: YoloHyperparametersValues;
  onChange: <K extends keyof YoloHyperparametersValues>(
    key: K,
    val: YoloHyperparametersValues[K],
  ) => void;
  disabled?: boolean;
  firstSelectRef?: React.Ref<SelectRefHandle>;
  epochsError?: string | null;
  lr0Error?: string | null;
}

export function YoloHyperparameters({
  values,
  onChange,
  disabled = false,
  firstSelectRef,
  epochsError,
  lr0Error,
}: YoloHyperparametersProps) {
  const modelOptions = useMemo<SelectOption<string>[]>(() => {
    return YOLO_MODELS.map((m) => {
      let badgeText: string | undefined;
      if (m === "yolo11n") badgeText = "Nano · Ultraleve";
      else if (m === "yolo11m") badgeText = "Médio · Padrão";
      else if (m === "yolo11x") badgeText = "Extra · Alta VRAM";
      else if (m === "yolo11-seg") badgeText = "Segmentação";
      return {
        value: m,
        label: m,
        badge: badgeText ? (
          <span className="font-mono text-2xs text-zinc-400">
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

  return (
    <div className="space-y-3.5">
      {/* Modelo e Épocas */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div>
          <label
            htmlFor="yolo-model-select"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Modelo Base
          </label>
          <Select
            ref={firstSelectRef}
            id="yolo-model-select"
            size="default"
            value={values.model}
            onChange={(val) => onChange("model", val)}
            options={modelOptions}
            disabled={disabled}
          />
        </div>

        <div>
          <label
            htmlFor="yolo-epochs-input"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Épocas ({EPOCHS_MIN}-{EPOCHS_MAX})
          </label>
          <Input
            id="yolo-epochs-input"
            type="number"
            min={EPOCHS_MIN}
            max={EPOCHS_MAX}
            value={values.epochs}
            onChange={(e) => {
              const v = parseInt(e.target.value, 10);
              onChange("epochs", isNaN(v) ? 0 : v);
            }}
            disabled={disabled}
            error={epochsError || undefined}
          />
        </div>
      </div>

      {/* Batch e Resolução */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div>
          <label
            htmlFor="yolo-batch-select"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Lote (Batch)
          </label>
          <Select
            id="yolo-batch-select"
            size="default"
            value={values.batch}
            onChange={(val) => onChange("batch", val)}
            options={batchOptions}
            disabled={disabled}
          />
        </div>

        <div>
          <label
            htmlFor="yolo-imgsz-select"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Resolução (imgsz)
          </label>
          <Select
            id="yolo-imgsz-select"
            size="default"
            value={values.imgsz}
            onChange={(val) => onChange("imgsz", val)}
            options={imgszOptions}
            disabled={disabled}
          />
        </div>
      </div>

      {/* lr0 e Otimizador */}
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div>
          <label
            htmlFor="yolo-lr0-input"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Taxa Inicial (lr0)
          </label>
          <Input
            id="yolo-lr0-input"
            type="text"
            value={values.lr0}
            onChange={(e) => onChange("lr0", e.target.value)}
            disabled={disabled}
            placeholder="0.01"
            error={lr0Error || undefined}
          />
        </div>

        <div>
          <label
            htmlFor="yolo-optimizer-select"
            className="mb-1 block font-mono text-2xs uppercase tracking-caps text-zinc-400"
          >
            Otimizador
          </label>
          <Select
            id="yolo-optimizer-select"
            size="default"
            value={values.optimizer}
            onChange={(val) => onChange("optimizer", val)}
            options={optimizerOptions}
            disabled={disabled}
          />
        </div>
      </div>

      {/* Aumentação de Dados */}
      <div className="rounded-xl border border-white/5 bg-white/[0.02] p-3 space-y-2">
        <span className="font-mono text-2xs uppercase tracking-caps text-zinc-400 block font-semibold">
          Aumentação de Dados (Data Augmentation)
        </span>
        <div className="grid grid-cols-2 gap-2 text-xs">
          <label className="flex items-center space-x-2 cursor-pointer">
            <input
              type="checkbox"
              checked={values.augment.mosaic}
              disabled={disabled}
              onChange={(e) =>
                onChange("augment", {
                  ...values.augment,
                  mosaic: e.target.checked,
                })
              }
              className="rounded border-zinc-700 bg-zinc-900 text-brand-500 focus:ring-brand-500/40"
            />
            <span className="text-zinc-300">Mosaic</span>
          </label>
          <label className="flex items-center space-x-2 cursor-pointer">
            <input
              type="checkbox"
              checked={values.augment.mixupFlip}
              disabled={disabled}
              onChange={(e) =>
                onChange("augment", {
                  ...values.augment,
                  mixupFlip: e.target.checked,
                })
              }
              className="rounded border-zinc-700 bg-zinc-900 text-brand-500 focus:ring-brand-500/40"
            />
            <span className="text-zinc-300">Mixup & Flip</span>
          </label>
        </div>
      </div>
    </div>
  );
}

export default YoloHyperparameters;
