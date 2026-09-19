"use client";

import type { Dispatch, SetStateAction } from "react";
import { IconSettings } from "@/components/icons";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption } from "@/components/ui/Select";
import {
  DIFFUSION_EPOCHS_MAX,
  DIFFUSION_EPOCHS_MIN,
  type DiffusionHyperparametersValues,
} from "./estimateDiffusionVram";

export interface DiffusionHyperparametersSectionProps {
  params: DiffusionHyperparametersValues;
  setParams: Dispatch<SetStateAction<DiffusionHyperparametersValues>>;
  outputName: string;
  setOutputName: (val: string) => void;
  defaultSuggestedOutputName: string;
  busy: boolean;
  batchOptions: SelectOption<number>[];
  rankOptions: SelectOption<number>[];
}

export function DiffusionHyperparametersSection({
  params,
  setParams,
  outputName,
  setOutputName,
  defaultSuggestedOutputName,
  busy,
  batchOptions,
  rankOptions,
}: DiffusionHyperparametersSectionProps) {
  return (
    <>
      {/* Trigger Word */}
      <div className="space-y-1.5">
        <label
          htmlFor="trigger-word"
          className="block text-xs font-medium text-zinc-300"
        >
          Trigger Word (Palavra-Gatilho)
        </label>
        <Input
          id="trigger-word"
          type="text"
          placeholder="ex: ohwx person, sks style, vintage anime"
          value={params.triggerWord}
          onChange={(e) =>
            setParams((p) => ({ ...p, triggerWord: e.target.value }))
          }
          disabled={busy}
          className="font-mono text-xs"
        />
        <p className="text-2xs font-mono text-zinc-500">
          Opcional. Prefixa automaticamente as legendas de cada imagem durante o
          empacotamento.
        </p>
      </div>

      {/* Nome do Adaptador / Modelo (outputName — ADR-0022 D1/D4) */}
      <div className="space-y-1.5">
        <label
          htmlFor="diffusion-output-name"
          className="block text-xs font-medium text-zinc-300"
        >
          Nome do Adaptador / Modelo{" "}
          <span className="text-zinc-500 font-normal">(opcional)</span>
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
        <p className="text-2xs font-mono text-zinc-500">
          Nome personalizado para o arquivo .safetensors. Se omitido, o estúdio
          gerará um nome semântico inteligente.
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
              setParams((p) => ({
                ...p,
                epochs: parseInt(e.target.value, 10) || 1,
              }))
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
            onChange={(val) =>
              setParams((p) => ({ ...p, batchSize: Number(val) }))
            }
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
            onChange={(e) =>
              setParams((p) => ({ ...p, learningRate: e.target.value }))
            }
            disabled={busy}
            fontMono
            className="h-[38px] text-xs font-mono"
          />
        </div>
      </div>
    </>
  );
}
