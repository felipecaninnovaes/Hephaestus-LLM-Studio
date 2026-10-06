"use client";

import { IconSliders, IconZap } from "@/components/icons";
import { LoRAEditor } from "@/components/studio/LoRAEditor";
import {
  SegmentedControl,
  Select,
  type SelectOption,
} from "@/components/ui";
import type { LoraRef, Model } from "@/types/studio";

export interface GenerationModelSectionProps {
  modelSelectOptions: SelectOption<string>[];
  modelSelectValue: string;
  onModelSelectChange: (val: string) => void;
  disabled: boolean;
  loadingModels: boolean;
  checkpointModelsCount: number;
  customModelId: string;
  effectiveArch: string | null;
  textEncoderOptions: SelectOption<string>[];
  textEncoderModelId: string;
  setTextEncoderModelId: (val: string) => void;
  isFlux2: boolean;
  /** Regras exclusivas do 4B: text encoder custom e variante destilada. */
  isFlux2Only4b: boolean;
  distilled: boolean;
  onVariantChange: (variant: "distilled" | "base") => void;
  loras: LoraRef[];
  setLoras: (loras: LoraRef[]) => void;
  loraModels: Model[];
}

export function GenerationModelSection({
  modelSelectOptions,
  modelSelectValue,
  onModelSelectChange,
  disabled,
  loadingModels,
  checkpointModelsCount,
  customModelId,
  effectiveArch,
  textEncoderOptions,
  textEncoderModelId,
  setTextEncoderModelId,
  isFlux2,
  isFlux2Only4b,
  distilled,
  onVariantChange,
  loras,
  setLoras,
  loraModels,
}: GenerationModelSectionProps) {
  return (
    <div className="space-y-4">
      {/* ══ Modelo (presets oficiais + checkpoints custom por arch) ══ */}
      <div className="space-y-2">
        <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Modelo
        </span>
        <Select
          options={modelSelectOptions}
          value={modelSelectValue}
          onChange={onModelSelectChange}
          disabled={disabled}
          loading={loadingModels}
          loadingText="Carregando modelos…"
          placeholder={
            checkpointModelsCount === 0
              ? "Modelo oficial (envie customs em Modelos & Pesos)"
              : "Selecione o modelo"
          }
          size="sm"
          fontMono
        />
        {customModelId && effectiveArch && (
          <p className="font-mono text-3xs text-zinc-500">
            Checkpoint custom · arch {effectiveArch}
          </p>
        )}
      </div>

      {/* ══ Text encoder (somente flux-2-klein-4b; 9B usa o encoder oficial) ══ */}
      <div className="space-y-2">
        <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Text encoder
        </span>
        <Select
          options={textEncoderOptions}
          value={isFlux2Only4b ? textEncoderModelId : ""}
          onChange={(v) => setTextEncoderModelId(v)}
          disabled={disabled || !isFlux2Only4b}
          placeholder={
            isFlux2Only4b
              ? "Encoder oficial BFL (padrão)"
              : "Disponível só p/ FLUX.2 Klein 4B"
          }
          size="sm"
          fontMono
        />
        {!isFlux2Only4b && (
          <p className="font-mono text-3xs text-zinc-500">
            Text encoder custom só tem efeito com arch flux-2-klein-4b.
          </p>
        )}
      </div>

      {/* ══ Variante FLUX (destilada/base) — SegmentedControl canônico ══ */}
      {isFlux2 && (
        <div className="space-y-2 rounded-xl border border-white/8 bg-white/[0.02] p-3">
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
            <span className="font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-300">
              Variante
            </span>
            <span className="shrink-0 font-mono text-3xs text-brand-400">
              {!isFlux2Only4b
                ? "28 steps · CFG 4 · só base"
                : distilled
                  ? "4–8 steps · CFG 1.0"
                  : "20+ steps · CFG 3.5+"}
            </span>
          </div>
          <SegmentedControl
            options={[
              {
                id: "distilled",
                label: "Destilado",
                icon: <IconZap className="size-3.5" />,
              },
              {
                id: "base",
                label: "Base",
                icon: <IconSliders className="size-3.5" />,
              },
            ]}
            value={isFlux2Only4b && distilled ? "distilled" : "base"}
            onChange={onVariantChange}
            disabled={disabled || !isFlux2Only4b}
            ariaLabel="Variante do modelo"
          />
        </div>
      )}

      {/* ══ Multi-LoRA ══ */}
      <LoRAEditor
        value={loras}
        onChange={setLoras}
        loraModels={loraModels}
        disabled={disabled}
      />
    </div>
  );
}
