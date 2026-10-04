"use client";

import { useEffect, useMemo, useState } from "react";
import {
  IconColumns,
  IconDownload,
  IconLayers,
  IconZoomIn,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { ImageSplitCompare } from "@/components/ui/ImageSplitCompare";
import { Slider } from "@/components/ui/Slider";
import type { SampleStep } from "@/lib/sampleArtifacts";
import type { JobArtifact } from "@/types/jobs";
import { SampleCard } from "./SampleCard";
interface SampleStepGalleryProps {
  jobId: string;
  steps: SampleStep[];
  onZoom: (art: JobArtifact) => void;
  onDownload?: (jobId: string, art: JobArtifact) => void;
}

function StepPane({
  jobId,
  steps,
  position,
  onPositionChange,
  label,
  onZoom,
  onDownload,
}: {
  jobId: string;
  steps: SampleStep[];
  position: number;
  onPositionChange: (pos: number) => void;
  label: string;
  onZoom: (art: JobArtifact) => void;
  onDownload?: (jobId: string, art: JobArtifact) => void;
}) {
  const step = steps[position];
  const gridClass =
    !step || step.groups.length <= 1
      ? "grid-cols-1"
      : step.groups.length === 2
        ? "grid-cols-2"
        : "grid-cols-2 sm:grid-cols-3";
  return (
    <div className="space-y-2">
      <Slider
        label={label}
        aria-label={label}
        value={position}
        min={0}
        max={Math.max(0, steps.length - 1)}
        step={1}
        onChange={onPositionChange}
        formatValue={() => step?.label ?? ""}
        aria-valuetext={step?.label ?? ""}
      />
      {step && (
        <div className={`grid gap-2 ${gridClass}`}>
          {step.groups.map((g) => (
            <SampleCard
              key={g.art.id}
              jobId={jobId}
              art={g.art}
              onZoom={onZoom}
              onDownload={onDownload}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function SampleStepGallery({
  jobId,
  steps,
  onZoom,
  onDownload,
}: SampleStepGalleryProps) {
  const lastIndex = Math.max(0, steps.length - 1);
  const [posA, setPosA] = useState(lastIndex);
  const [posB, setPosB] = useState(0);
  const [compare, setCompare] = useState(false);
  const [orientation, setOrientation] = useState<"horizontal" | "vertical">(
    "horizontal",
  );
  const [selectedGroupIndex, setSelectedGroupIndex] = useState(0);

  // Acompanha novas amostras chegando ao vivo: mantém a posição A grudada no
  // step mais recente enquanto o usuário não navegou manualmente para trás.
  useEffect(() => {
    setPosA((prev) => {
      const sticky =
        prev === lastIndex - 1 || prev === lastIndex ? lastIndex : prev;
      return Math.min(Math.max(sticky, 0), lastIndex);
    });
    setPosB((prev) => Math.min(Math.max(prev, 0), lastIndex));
  }, [lastIndex]);

  const stepA = steps[posA];
  const stepB = steps[posB];

  // Grupos / prompts disponíveis para comparação
  const availableGroups = useMemo(() => {
    if (!stepA && !stepB) return [];
    const keysA = stepA?.groups.map((g) => g.promptKey) ?? [];
    const keysB = stepB?.groups.map((g) => g.promptKey) ?? [];
    const allKeys = Array.from(new Set([...keysA, ...keysB]));
    return allKeys;
  }, [stepA, stepB]);

  // Garante que o índice de grupo selecionado está dentro dos limites
  const activeGroupKey = availableGroups[selectedGroupIndex] ?? availableGroups[0] ?? "default";

  // Encontra artefatos de A e B para o grupo selecionado (ou fallback para o primeiro grupo disponível)
  const artA: JobArtifact | undefined =
    stepA?.groups.find((g) => g.promptKey === activeGroupKey)?.art ??
    stepA?.groups[0]?.art;

  const artB: JobArtifact | undefined =
    stepB?.groups.find((g) => g.promptKey === activeGroupKey)?.art ??
    stepB?.groups[0]?.art;

  if (steps.length === 0) return null;

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2 flex-wrap">
        <span className="font-mono text-3xs uppercase tracking-caps text-zinc-500">
          Navegar por época ({steps.length})
        </span>
        <div className="flex items-center gap-2">
          {compare && (
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() =>
                setOrientation((o) =>
                  o === "horizontal" ? "vertical" : "horizontal",
                )
              }
              title={
                orientation === "horizontal"
                  ? "Alternar para divisor horizontal (corte cima/baixo)"
                  : "Alternar para divisor vertical (corte esquerda/direita)"
              }
              aria-label={
                orientation === "horizontal"
                  ? "Alternar para divisor horizontal"
                  : "Alternar para divisor vertical"
              }
              leftIcon={
                <IconColumns
                  className={`size-3.5 transition-transform duration-200 ${
                    orientation === "vertical" ? "rotate-90" : ""
                  }`}
                />
              }
            >
              <span>
                {orientation === "horizontal"
                  ? "Divisor vertical"
                  : "Divisor horizontal"}
              </span>
            </Button>
          )}

          {steps.length > 1 && (
            <label className="inline-flex min-h-8 cursor-pointer items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 font-mono text-2xs text-zinc-300 transition hover:border-indigo-500/40 hover:bg-white/[0.06] hover:text-white">
              <input
                type="checkbox"
                checked={compare}
                onChange={(e) => setCompare(e.target.checked)}
                className="size-3.5 accent-brand-500"
              />
              <IconLayers className="size-3.5" />
              Comparar A/B
            </label>
          )}
        </div>
      </div>

      {!compare && (
        <div className="mx-auto w-full max-w-md">
          <StepPane
            jobId={jobId}
            steps={steps}
            position={posA}
            onPositionChange={setPosA}
            label="Posição"
            onZoom={onZoom}
            onDownload={onDownload}
          />
        </div>
      )}

      {compare && (
        <div className="space-y-4">
          {/* Controles de Época para A e B separados e claros */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="p-3 rounded-xl border border-white/10 bg-white/[0.02]">
              <Slider
                label="Amostra A"
                aria-label="Amostra A"
                value={posA}
                min={0}
                max={Math.max(0, steps.length - 1)}
                step={1}
                onChange={setPosA}
                formatValue={() => stepA?.label ?? ""}
                aria-valuetext={stepA?.label ?? ""}
              />
            </div>
            <div className="p-3 rounded-xl border border-white/10 bg-white/[0.02]">
              <Slider
                label="Amostra B"
                aria-label="Amostra B"
                value={posB}
                min={0}
                max={Math.max(0, steps.length - 1)}
                step={1}
                onChange={setPosB}
                formatValue={() => stepB?.label ?? ""}
                aria-valuetext={stepB?.label ?? ""}
              />
            </div>
          </div>

          {/* Seletor de Prompt/Grupo se houver mais de 1 */}
          {availableGroups.length > 1 && (
            <div className="flex items-center gap-2 overflow-x-auto pb-1">
              <span className="font-mono text-3xs uppercase tracking-caps text-zinc-400 shrink-0">
                Grupo:
              </span>
              {availableGroups.map((gKey, idx) => (
                <button
                  key={gKey}
                  type="button"
                  onClick={() => setSelectedGroupIndex(idx)}
                  className={`rounded-md px-2 py-1 font-mono text-3xs transition cursor-pointer shrink-0 ${
                    idx === selectedGroupIndex
                      ? "border border-brand-500/50 bg-brand-500/20 text-white font-semibold"
                      : "border border-white/10 bg-white/[0.03] text-zinc-400 hover:text-zinc-200"
                  }`}
                >
                  {gKey === "default" ? "Prompt Principal" : gKey}
                </button>
              ))}
            </div>
          )}

          {/* Quadro Split A/B */}
          {artA && artB ? (
            <div className="mx-auto w-full max-w-xl space-y-2">
              <ImageSplitCompare
                beforeSrc={`/api/jobs/${jobId}/artifacts/${artA.id}/data`}
                beforeAlt={stepA?.label ?? "A"}
                beforeLabel={`A: ${stepA?.label ?? ""}`}
                afterSrc={`/api/jobs/${jobId}/artifacts/${artB.id}/data`}
                afterAlt={stepB?.label ?? "B"}
                afterLabel={`B: ${stepB?.label ?? ""}`}
                orientation={orientation}
                aspectRatioClass="aspect-square"
              />

              {/* Barra de ações / meta da comparação com espaçamento e quebra limpos */}
              <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-1 text-2xs text-zinc-400 font-mono">
                <span className="shrink-0 text-zinc-400">
                  {orientation === "horizontal"
                    ? "Arraste esquerda ↔ direita ou use as setas"
                    : "Arraste cima ↕ baixo ou use as setas"}
                </span>
                <div className="flex items-center gap-2.5 shrink-0">
                  <button
                    type="button"
                    onClick={() => onZoom(artA)}
                    title={`Ampliar amostra A (${stepA?.label})`}
                    className="inline-flex items-center gap-1 hover:text-zinc-200 transition cursor-pointer"
                  >
                    <IconZoomIn className="size-3 text-brand-400" />
                    <span>Zoom A</span>
                  </button>
                  <span className="text-zinc-600">·</span>
                  <button
                    type="button"
                    onClick={() => onZoom(artB)}
                    title={`Ampliar amostra B (${stepB?.label})`}
                    className="inline-flex items-center gap-1 hover:text-zinc-200 transition cursor-pointer"
                  >
                    <IconZoomIn className="size-3 text-brand-400" />
                    <span>Zoom B</span>
                  </button>
                  {onDownload && (
                    <>
                      <span className="text-zinc-600">·</span>
                      <button
                        type="button"
                        onClick={() => onDownload(jobId, artA)}
                        title={`Baixar amostra A (${stepA?.label})`}
                        className="inline-flex items-center gap-1 hover:text-zinc-200 transition cursor-pointer"
                      >
                        <IconDownload className="size-3 text-zinc-400" />
                        <span>Baixar A</span>
                      </button>
                    </>
                  )}
                </div>
              </div>
            </div>
          ) : (
            <div className="p-8 text-center text-xs text-zinc-500 font-mono">
              Não há amostras suficientes para comparação direta neste grupo.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
