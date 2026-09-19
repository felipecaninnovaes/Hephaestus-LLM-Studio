"use client";

import type { ChangeEvent, RefObject } from "react";
import {
  IconDownload,
  IconSparkles,
  IconUpload,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import type { DiffusionPreset } from "@/types/studio";

export interface DiffusionPresetBarProps {
  busy: boolean;
  fileInputRef: RefObject<HTMLInputElement | null>;
  onApplyPreset: (preset: Partial<DiffusionPreset> & { name: string }) => void;
  onExportPreset: () => void;
  onImportPreset: (e: ChangeEvent<HTMLInputElement>) => void;
}

export function DiffusionPresetBar({
  busy,
  fileInputRef,
  onApplyPreset,
  onExportPreset,
  onImportPreset,
}: DiffusionPresetBarProps) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3 backdrop-blur-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <IconSparkles className="size-3.5 text-brand-400" />
          <span className="font-display text-xs font-semibold text-zinc-200">
            Presets de Treinamento
          </span>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="file"
            ref={fileInputRef}
            onChange={onImportPreset}
            accept=".json,application/json"
            className="hidden"
          />
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => fileInputRef.current?.click()}
            disabled={busy}
            leftIcon={<IconUpload className="size-3" />}
            className="font-mono text-2xs"
          >
            Importar JSON
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onExportPreset}
            disabled={busy}
            leftIcon={<IconDownload className="size-3" />}
            className="font-mono text-2xs"
          >
            Exportar JSON
          </Button>
        </div>
      </div>

      {/* Botões de presets rápidos */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={() =>
            onApplyPreset({
              name: "FLUX.2 Klein 4B (4-bit NF4)",
              baseModel: "flux",
              epochs: 10,
              batchSize: 1,
              rank: 16,
              alpha: 16,
              learningRate: "0.00003",
              resolution: 1024,
              gradientAccumulationSteps: 1,
              optimizer: "paged_adamw8bit",
              lrScheduler: "cosine",
              lrWarmupSteps: 0,
              mixedPrecision: "bf16",
              quantization: "4bit",
            })
          }
          className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
        >
          <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
            FLUX.2 Klein 4B
          </span>
          <span className="font-mono text-3xs text-zinc-400 mt-0.5">
            1024px · QLoRA 4-bit · ~8-10GB
          </span>
        </button>

        <button
          type="button"
          disabled={busy}
          onClick={() =>
            onApplyPreset({
              name: "Equilibrado (SDXL 1024)",
              baseModel: "sdxl",
              epochs: 10,
              batchSize: 1,
              rank: 16,
              alpha: 16,
              learningRate: "0.0001",
              resolution: 1024,
              gradientAccumulationSteps: 1,
              optimizer: "paged_adamw8bit",
              lrScheduler: "cosine",
              lrWarmupSteps: 0,
              mixedPrecision: "fp16",
              quantization: "4bit",
            })
          }
          className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
        >
          <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
            SDXL Padrão
          </span>
          <span className="font-mono text-3xs text-zinc-400 mt-0.5">
            1024px · QLoRA SDXL · ~8GB
          </span>
        </button>

        <button
          type="button"
          disabled={busy}
          onClick={() =>
            onApplyPreset({
              name: "Eco VRAM (SD 1.5 512)",
              baseModel: "sd15",
              epochs: 10,
              batchSize: 1,
              rank: 8,
              alpha: 8,
              learningRate: "0.0001",
              resolution: 512,
              gradientAccumulationSteps: 2,
              optimizer: "paged_adamw8bit",
              lrScheduler: "cosine",
              lrWarmupSteps: 0,
              mixedPrecision: "fp16",
              quantization: "4bit",
            })
          }
          className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
        >
          <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
            Eco 5 GB (SD 1.5)
          </span>
          <span className="font-mono text-3xs text-zinc-400 mt-0.5">
            512px · QLoRA 4-bit · GA 2x
          </span>
        </button>

        <button
          type="button"
          disabled={busy}
          onClick={() =>
            onApplyPreset({
              name: "Alta Fidelidade (Rank 32)",
              baseModel: "sdxl",
              epochs: 15,
              batchSize: 1,
              rank: 32,
              alpha: 32,
              learningRate: "0.00005",
              resolution: 1024,
              gradientAccumulationSteps: 2,
              optimizer: "paged_adamw8bit",
              lrScheduler: "cosine",
              lrWarmupSteps: 50,
              mixedPrecision: "fp16",
              quantization: "4bit",
            })
          }
          className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
        >
          <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
            Alta Fidelidade
          </span>
          <span className="font-mono text-3xs text-zinc-400 mt-0.5">
            1024px · Rank 32 · GA 2x
          </span>
        </button>

        <button
          type="button"
          disabled={busy}
          onClick={() =>
            onApplyPreset({
              name: "Prodigy Adaptativo",
              baseModel: "sdxl",
              epochs: 10,
              batchSize: 1,
              rank: 16,
              alpha: 16,
              learningRate: "1.0",
              resolution: 1024,
              gradientAccumulationSteps: 2,
              optimizer: "prodigy",
              lrScheduler: "cosine",
              lrWarmupSteps: 50,
              mixedPrecision: "fp16",
              quantization: "4bit",
            })
          }
          className="flex flex-col text-left p-2.5 rounded-lg border border-white/10 bg-white/[0.02] hover:border-brand-500/40 hover:bg-white/[0.05] transition-colors text-zinc-300 group focus:outline-none focus:ring-2 focus:ring-brand-500/40 cursor-pointer"
        >
          <span className="font-display text-xs font-medium text-zinc-200 group-hover:text-brand-300 transition-colors">
            Auto LR Prodigy
          </span>
          <span className="font-mono text-3xs text-zinc-400 mt-0.5">
            Adaptativo · LR 1.0 auto
          </span>
        </button>
      </div>
    </div>
  );
}
