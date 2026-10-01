"use client";

import { useEffect, useMemo, useState } from "react";
import { IconDownload, IconImage, IconX } from "@/components/icons";
import { formatBytes } from "@/lib/format";
import { buildSampleTimeline, formatSampleLabel } from "@/lib/sampleArtifacts";
import type { JobArtifact } from "@/types/jobs";
import { SampleCard } from "./SampleCard";
import { SampleStepGallery } from "./SampleStepGallery";

interface JobSamplesGalleryProps {
  jobId: string;
  artifacts: JobArtifact[];
  onDownload?: (jobId: string, art: JobArtifact) => void;
  className?: string;
}

export function JobSamplesGallery({
  jobId,
  artifacts,
  onDownload,
  className = "",
}: JobSamplesGalleryProps) {
  const [selectedSample, setSelectedSample] = useState<JobArtifact | null>(null);

  useEffect(() => {
    if (!selectedSample) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setSelectedSample(null);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedSample]);

  // Timeline por época (baseline → final) + amostras sem época reconhecível,
  // derivada por `lib/sampleArtifacts.ts` a partir dos nomes reais emitidos
  // pelo trainer-difusao (`samples/sample_epoch_NNN.png`).
  const { steps, untagged } = useMemo(() => buildSampleTimeline(artifacts), [artifacts]);

  const total = steps.reduce((acc, s) => acc + s.groups.length, 0) + untagged.length;
  if (total === 0) return null;

  return (
    <div className={`space-y-3 ${className}`}>
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 font-mono text-3xs uppercase tracking-caps text-zinc-400">
          <IconImage className="size-3.5 text-indigo-400" />
          Amostras de Validação ({total})
        </span>
        <span className="font-mono text-3xs text-zinc-400">LoRA Diffusion Previews</span>
      </div>

      {steps.length > 0 && (
        <SampleStepGallery
          jobId={jobId}
          steps={steps}
          onZoom={setSelectedSample}
          onDownload={onDownload}
        />
      )}

      {untagged.length > 0 && (
        <div className="space-y-1.5">
          <span className="block font-mono text-3xs uppercase tracking-caps text-zinc-500">
            Outras amostras ({untagged.length})
          </span>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            {untagged.map((art) => (
              <SampleCard
                key={art.id}
                jobId={jobId}
                art={art}
                onZoom={setSelectedSample}
                onDownload={onDownload}
              />
            ))}
          </div>
        </div>
      )}

      {/* Lightbox Modal para ampliação de alta resolução */}
      {selectedSample && (
        // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop suplementar — há botão fechar explícito e focável; o backdrop fica fora da tab-order de propósito.
        <div
          role="dialog"
          aria-modal="true"
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-4 backdrop-blur-md animate-fadeIn"
          onClick={(e) => {
            if (e.target === e.currentTarget) setSelectedSample(null);
          }}
        >
          <div className="relative w-full max-w-2xl space-y-3 overflow-hidden rounded-2xl border border-white/15 bg-zinc-900/95 p-4 shadow-2xl backdrop-blur-xl">
            <div className="flex items-center justify-between border-b border-white/10 pb-3">
              <div className="flex items-center gap-2">
                <span className="font-mono text-sm font-semibold text-zinc-100">
                  {formatSampleLabel(selectedSample.path)}
                </span>
                <span className="font-mono text-2xs text-zinc-400">
                  · {selectedSample.path.split("/").pop()} · {formatBytes(selectedSample.bytes)}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setSelectedSample(null)}
                className="min-h-8 min-w-8 rounded-lg p-1 text-zinc-400 transition hover:bg-white/10 hover:text-zinc-100"
              >
                <IconX className="size-4" />
              </button>
            </div>

            <div className="flex max-h-[70vh] justify-center overflow-hidden rounded-xl border border-white/5 bg-black/60">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`/api/jobs/${jobId}/artifacts/${selectedSample.id}/data`}
                alt={selectedSample.path}
                className="max-h-[70vh] w-auto object-contain"
              />
            </div>

            <div className="flex items-center justify-between pt-2">
              <span className="font-mono text-2xs text-zinc-400">
                Amostra sintetizada por Diffusers LoRA
              </span>
              {onDownload && (
                <button
                  type="button"
                  onClick={() => onDownload(jobId, selectedSample)}
                  className="inline-flex cursor-pointer items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 font-mono text-xs font-medium text-white shadow-sm transition hover:bg-indigo-500"
                >
                  <IconDownload className="size-3.5" />
                  Baixar Imagem
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
