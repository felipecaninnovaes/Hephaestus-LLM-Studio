"use client";

import { IconImage } from "@/components/icons";
import { Input } from "@/components/ui/Input";

export interface DiffusionSamplesSectionProps {
  enableSamples: boolean;
  setEnableSamples: (val: boolean) => void;
  samplePrompt: string;
  setSamplePrompt: (val: string) => void;
  sampleInterval: number;
  setSampleInterval: (val: number) => void;
  sampleSeed: string;
  setSampleSeed: (val: string) => void;
  triggerWord: string;
  epochs: number;
  busy: boolean;
}

export function DiffusionSamplesSection({
  enableSamples,
  setEnableSamples,
  samplePrompt,
  setSamplePrompt,
  sampleInterval,
  setSampleInterval,
  sampleSeed,
  setSampleSeed,
  triggerWord,
  epochs,
  busy,
}: DiffusionSamplesSectionProps) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3 backdrop-blur-sm">
      <div className="flex items-center justify-between">
        <label
          htmlFor="enable-samples-toggle"
          className="flex items-center gap-2 cursor-pointer select-none"
        >
          <input
            id="enable-samples-toggle"
            type="checkbox"
            checked={enableSamples}
            onChange={(e) => setEnableSamples(e.target.checked)}
            disabled={busy}
            className="size-4 rounded border-white/20 bg-white/5 text-brand-500 focus:ring-brand-500/30"
          />
          <span className="font-mono text-xs font-semibold text-zinc-200 flex items-center gap-1.5">
            <IconImage className="size-3.5 text-brand-400" />
            Amostras Visuais de Validação
          </span>
        </label>
        <span className="font-mono text-3xs text-zinc-400">
          {enableSamples ? "Ativado" : "Desativado"}
        </span>
      </div>

      {enableSamples && (
        <div className="space-y-3 pt-1 border-t border-white/5">
          <Input
            id="sample-prompt-input"
            label="Prompt de Teste para Amostras"
            hint="Uma imagem será sintetizada para você acompanhar a evolução visual no Action Center e página de jobs."
            type="text"
            value={samplePrompt}
            onChange={(e) => setSamplePrompt(e.target.value)}
            placeholder={
              triggerWord.trim()
                ? `ex: a photo of ${triggerWord.trim()} subject in studio lighting`
                : "ex: a photo of a cute robot in cinematic lighting, 8k"
            }
            disabled={busy}
            fontMono
            className="h-[38px] text-xs font-mono"
          />

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <Input
              id="sample-interval-input"
              label="Intervalo (a cada N épocas)"
              type="number"
              min={1}
              max={epochs}
              value={sampleInterval}
              onChange={(e) =>
                setSampleInterval(
                  Math.max(1, parseInt(e.target.value, 10) || 1),
                )
              }
              disabled={busy}
              fontMono
              className="h-[38px] text-xs font-mono"
            />
            <Input
              id="sample-seed-input"
              label="Seed da Amostra (Fixa)"
              hint="Fixa o ruído para comparar a evolução sobre a mesma composição."
              type="number"
              min={0}
              value={sampleSeed}
              onChange={(e) => setSampleSeed(e.target.value)}
              placeholder="42"
              disabled={busy}
              fontMono
              className="h-[38px] text-xs font-mono"
            />
          </div>
        </div>
      )}
    </div>
  );
}
