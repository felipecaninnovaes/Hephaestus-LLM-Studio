"use client";

import { useEffect, useState } from "react";
import { IconLayers } from "@/components/icons";
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
  return (
    <div className="space-y-2">
      <Slider
        label={label}
        value={position}
        min={0}
        max={Math.max(0, steps.length - 1)}
        step={1}
        onChange={onPositionChange}
        formatValue={() => step?.label ?? ""}
        aria-valuetext={step?.label ?? ""}
      />
      {step && (
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
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

export function SampleStepGallery({ jobId, steps, onZoom, onDownload }: SampleStepGalleryProps) {
  const lastIndex = Math.max(0, steps.length - 1);
  const [posA, setPosA] = useState(lastIndex);
  const [posB, setPosB] = useState(0);
  const [compare, setCompare] = useState(false);

  // Acompanha novas amostras chegando ao vivo: mantém a posição A grudada no
  // step mais recente enquanto o usuário não navegou manualmente para trás.
  useEffect(() => {
    setPosA((prev) => {
      const sticky = prev === lastIndex - 1 || prev === lastIndex ? lastIndex : prev;
      return Math.min(Math.max(sticky, 0), lastIndex);
    });
    setPosB((prev) => Math.min(Math.max(prev, 0), lastIndex));
  }, [lastIndex]);

  if (steps.length === 0) return null;

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <span className="font-mono text-3xs uppercase tracking-caps text-zinc-500">
          Navegar por época ({steps.length})
        </span>
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

      {!compare && (
        <StepPane
          jobId={jobId}
          steps={steps}
          position={posA}
          onPositionChange={setPosA}
          label="Posição"
          onZoom={onZoom}
          onDownload={onDownload}
        />
      )}

      {compare && (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          <StepPane
            jobId={jobId}
            steps={steps}
            position={posA}
            onPositionChange={setPosA}
            label="A"
            onZoom={onZoom}
            onDownload={onDownload}
          />
          <StepPane
            jobId={jobId}
            steps={steps}
            position={posB}
            onPositionChange={setPosB}
            label="B"
            onZoom={onZoom}
            onDownload={onDownload}
          />
        </div>
      )}
    </div>
  );
}
