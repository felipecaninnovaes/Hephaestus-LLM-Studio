"use client";

import { IconDice, IconPlay, IconRefresh, IconStop } from "@/components/icons";
import NodeSelect from "@/components/studio/NodeSelect";
import {
  Button,
  Kbd,
  SegmentedControl,
  Select,
  type SelectOption,
  Slider,
} from "@/components/ui";
import type { GeracaoQuantization, GeracaoSampler, GeracaoUpscaleModel } from "@/lib/geracao-storage";
import {
  ASPECT_RESOLUTION_PRESETS,
  QUANTIZATION_OPTIONS,
  SAMPLER_LABELS,
  SQUARE_RESOLUTION_OPTIONS,
  UPSCALE_MODEL_OPTIONS,
} from "./generationTypes";

export interface GenerationParametersCardProps {
  width: number;
  height: number;
  setWidth: (w: number) => void;
  setHeight: (h: number) => void;
  steps: number;
  setSteps: (s: number) => void;
  guidanceScale: number;
  setGuidanceScale: (g: number) => void;
  isLockedSeed: boolean;
  setIsLockedSeed: (locked: boolean) => void;
  seed: number;
  setSeed: (s: number) => void;
  onRollSeed: () => void;
  sampler: GeracaoSampler;
  setSampler: (s: GeracaoSampler) => void;
  samplerOptions: SelectOption<string>[];
  quantization: GeracaoQuantization;
  setQuantization: (q: GeracaoQuantization) => void;
  upscaleEnabled: boolean;
  setUpscaleEnabled: (enabled: boolean) => void;
  upscaleModel: GeracaoUpscaleModel;
  setUpscaleModel: (m: GeracaoUpscaleModel) => void;
  upscaleScale: 2 | 4;
  setUpscaleScale: (s: 2 | 4) => void;
  batchSize: number;
  setBatchSize: (b: number) => void;
  selectedOrchestratorId: string | null;
  setSelectedOrchestratorId: (id: string | null) => void;
  isBusy: boolean;
  activeJobId: string | null;
  onGenerate: () => void;
  onCancelJob: () => void;
  onResetForm: () => void;
}

export function GenerationParametersCard({
  width,
  height,
  setWidth,
  setHeight,
  steps,
  setSteps,
  guidanceScale,
  setGuidanceScale,
  isLockedSeed,
  setIsLockedSeed,
  seed,
  setSeed,
  onRollSeed,
  sampler,
  setSampler,
  samplerOptions,
  quantization,
  setQuantization,
  upscaleEnabled,
  setUpscaleEnabled,
  upscaleModel,
  setUpscaleModel,
  upscaleScale,
  setUpscaleScale,
  batchSize,
  setBatchSize,
  selectedOrchestratorId,
  setSelectedOrchestratorId,
  isBusy,
  activeJobId,
  onGenerate,
  onCancelJob,
  onResetForm,
}: GenerationParametersCardProps) {
  return (
    <div className="space-y-4">
      {/* ══ Resolução — quadrada 256..2048 (Select com portal; sem overflow) ══ */}
      <div className="space-y-2">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
          <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
            Resolução
          </span>
          <span className="shrink-0 font-mono text-3xs text-zinc-400">
            {width}×{height}
          </span>
        </div>
        <Select
          options={[
            ...SQUARE_RESOLUTION_OPTIONS.map((r) => ({
              value: `${r}x${r}`,
              label: `${r}×${r}`,
            })),
            ...ASPECT_RESOLUTION_PRESETS.map((p) => ({
              value: `${p.width}x${p.height}`,
              label: p.label,
            })),
          ]}
          value={`${width}x${height}`}
          onChange={(v) => {
            const [w, h] = v.split("x").map(Number);
            setWidth(w);
            setHeight(h);
          }}
          disabled={isBusy}
          placeholder={`${width}×${height}`}
          size="sm"
          fontMono
        />
      </div>

      {/* ══ Steps — Slider canônico ══ */}
      <Slider
        label="Steps"
        value={steps}
        onChange={(v) => setSteps(Math.round(v))}
        min={1}
        max={50}
        step={1}
        disabled={isBusy}
        formatValue={(v) => String(Math.round(v))}
      />

      {/* ══ CFG — Slider canônico ══ */}
      <Slider
        label="CFG"
        value={guidanceScale}
        onChange={setGuidanceScale}
        min={1}
        max={15}
        step={0.5}
        disabled={isBusy}
        formatValue={(v) => v.toFixed(1)}
      />

      {/* ══ Seed — Input canônico + Button canônico ══ */}
      <div className="space-y-1.5">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
          <label
            htmlFor="gen-seed"
            className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300"
          >
            Seed
          </label>
          <SegmentedControl
            options={[
              { id: "auto", label: "Auto" },
              { id: "locked", label: "Travada" },
            ]}
            value={isLockedSeed ? "locked" : "auto"}
            onChange={(v) => setIsLockedSeed(v === "locked")}
            ariaLabel="Modo seed"
          />
        </div>
        <div className="flex items-center gap-2">
          <input
            id="gen-seed"
            type="number"
            value={seed}
            onChange={(e) => setSeed(parseInt(e.target.value, 10) || 0)}
            disabled={isBusy}
            className="flex-1 rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={onRollSeed}
            disabled={isBusy}
            title="Nova seed aleatória"
          >
            <IconDice className="size-4" />
          </Button>
        </div>
      </div>

      {/* ══ Sampler ══ */}
      <div className="space-y-2">
        <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Sampler
        </span>
        <Select
          options={samplerOptions}
          value={sampler}
          onChange={(v) => setSampler(v as GeracaoSampler)}
          disabled={isBusy}
          placeholder={SAMPLER_LABELS[sampler] ?? sampler}
          size="sm"
          fontMono
        />
      </div>

      {/* ══ Quantização ══ */}
      <div className="space-y-2">
        <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Quantização
        </span>
        <Select
          options={QUANTIZATION_OPTIONS}
          value={quantization}
          onChange={(v) => setQuantization(v as GeracaoQuantization)}
          disabled={isBusy}
          placeholder={
            QUANTIZATION_OPTIONS.find((o) => o.value === quantization)?.label ??
            quantization
          }
          size="sm"
          fontMono
        />
      </div>

      {/* ══ Upscale pós-geração ══ */}
      <div className="space-y-2 rounded-xl border border-white/8 bg-white/[0.02] p-3">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
          <label
            htmlFor="gen-upscale-toggle"
            className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300 cursor-pointer"
          >
            Upscale (Super-resolução)
          </label>
          <input
            id="gen-upscale-toggle"
            type="checkbox"
            checked={upscaleEnabled}
            onChange={(e) => setUpscaleEnabled(e.target.checked)}
            disabled={isBusy}
            className="size-3.5 rounded border-zinc-700 bg-black/40 text-brand-500 focus:ring-brand-500/50"
          />
        </div>
        {upscaleEnabled && (
          <div className="space-y-2 pt-1">
            <Select
              options={UPSCALE_MODEL_OPTIONS}
              value={upscaleModel}
              onChange={(v) => setUpscaleModel(v as GeracaoUpscaleModel)}
              disabled={isBusy}
              size="sm"
              fontMono
            />
            <div className="flex items-center justify-between gap-2">
              <span className="font-mono text-3xs text-zinc-400">Escala</span>
              <SegmentedControl
                options={[
                  { id: "2", label: "2x" },
                  { id: "4", label: "4x" },
                ]}
                value={String(upscaleScale)}
                onChange={(v) => setUpscaleScale(Number(v) as 2 | 4)}
                ariaLabel="Fator de escala do upscale"
              />
            </div>
            <p className="font-mono text-4xs text-zinc-400 leading-tight">
              A imagem final terá {width * upscaleScale}×{height * upscaleScale}
              px. Executado no nó após a difusão.
            </p>
          </div>
        )}
      </div>

      {/* ══ Batch size (gerar N imagens em paralelo) ══ */}
      <Slider
        label="Quantidade por lote"
        value={batchSize}
        onChange={(v) => setBatchSize(Math.round(v))}
        min={1}
        max={4}
        step={1}
        disabled={isBusy}
        formatValue={(v) => `${Math.round(v)} · seed +1 por imagem`}
      />

      {/* ══ Nó de Execução (Orchestrator) ══ */}
      <NodeSelect
        value={selectedOrchestratorId}
        onChange={setSelectedOrchestratorId}
        disabled={isBusy}
      />

      {/* ══ Botão Gerar / Cancelar ══ */}
      <div className="space-y-2 pt-2 border-t border-white/5">
        {activeJobId ? (
          <Button
            type="button"
            variant="destructive"
            size="lg"
            onClick={onCancelJob}
            className="w-full font-mono text-xs"
          >
            <IconStop className="size-4" />
            Cancelar Geração
          </Button>
        ) : (
          <Button
            type="button"
            variant="primary"
            size="lg"
            onClick={onGenerate}
            disabled={isBusy}
            loading={isBusy}
            className="w-full font-mono text-xs shadow-lg shadow-brand-500/20"
          >
            <IconPlay className="size-4" />
            Gerar {batchSize > 1 ? `(${batchSize} imagens)` : "Imagem"}
            <span className="ml-auto flex items-center gap-1 font-mono text-3xs text-zinc-400">
              <Kbd>Ctrl</Kbd>
              <Kbd>Enter</Kbd>
            </span>
          </Button>
        )}

        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={onResetForm}
          disabled={isBusy}
          className="w-full font-mono text-3xs text-zinc-500 hover:text-zinc-300"
        >
          <IconRefresh className="size-3" />
          Restaurar padrões
        </Button>
      </div>
    </div>
  );
}
