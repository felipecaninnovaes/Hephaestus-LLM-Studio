"use client";

import React, { useMemo, useState } from "react";
import { IconChevronDown, IconChevronRight, IconDownload, IconImage, IconX, IconZoomIn } from "@/components/icons";
import { formatBytes } from "@/lib/format";
import type { JobArtifact } from "@/types/studio";

interface JobSamplesGalleryProps {
  jobId: string;
  artifacts: JobArtifact[];
  onDownload?: (jobId: string, art: JobArtifact) => void;
  className?: string;
}

const COLLAPSED_EPOCH_COUNT = 6;

function parseSampleEpoch(path: string): number | null {
  const match = path.match(/epoch_(\d+)/i);
  return match ? parseInt(match[1], 10) : null;
}

// Helper para extrair o rótulo da época
function formatSampleLabel(path: string): string {
  const match = path.match(/epoch_(\d+)/i);
  if (match) {
    const ep = parseInt(match[1], 10);
    return ep === 0 ? "Baseline (Época 0)" : `Época ${ep}`;
  }
  const fname = path.split("/").pop() || path;
  return fname.replace(/\.[^/.]+$/, "");
}

export function JobSamplesGallery({
  jobId,
  artifacts,
  onDownload,
  className = "",
}: JobSamplesGalleryProps) {
  const [selectedSample, setSelectedSample] = useState<JobArtifact | null>(null);
  const [expanded, setExpanded] = useState(false);

  // Filtra apenas artefatos que são amostras geradas (kind === 'sample' ou path com 'samples/')
  // e ordena por época crescente: baseline (Época 0) primeiro, amostras sem época por último
  // preservando a ordem recebida do servidor.
  const { baselines, epochSamples, untaggedSamples } = useMemo(() => {
    const filtered = artifacts.filter(
      (art) =>
        art.kind === "sample" ||
        art.path.startsWith("samples/") ||
        art.path.includes("sample_epoch_") ||
        (art.path.endsWith(".png") && art.kind !== "model" && art.kind !== "metrics"),
    );
    const indexed = filtered.map((art, i) => ({ art, epoch: parseSampleEpoch(art.path), i }));
    indexed.sort((a, b) => {
      const ea = a.epoch;
      const eb = b.epoch;
      if (ea == null && eb == null) return a.i - b.i;
      if (ea == null) return 1;
      if (eb == null) return -1;
      if (ea !== eb) return ea - eb;
      return a.i - b.i;
    });
    return {
      baselines: indexed.filter((x) => x.epoch === 0).map((x) => x.art),
      epochSamples: indexed.filter((x) => x.epoch != null && x.epoch > 0).map((x) => x.art),
      untaggedSamples: indexed.filter((x) => x.epoch == null).map((x) => x.art),
    };
  }, [artifacts]);

  const total = baselines.length + epochSamples.length + untaggedSamples.length;
  if (total === 0) return null;

  const visibleEpochSamples = expanded
    ? epochSamples
    : epochSamples.slice(Math.max(0, epochSamples.length - COLLAPSED_EPOCH_COUNT));
  const canCollapse = epochSamples.length > COLLAPSED_EPOCH_COUNT;

  function renderSampleCard(art: JobArtifact) {
    const imgUrl = `/api/jobs/${jobId}/artifacts/${art.id}/data`;
    const label = formatSampleLabel(art.path);

    return (
      <div
        key={art.id}
        className="group relative rounded-xl overflow-hidden border border-white/10 bg-black/40 aspect-square flex flex-col justify-between transition-all hover:border-indigo-500/50 hover:shadow-lg hover:shadow-indigo-500/10"
      >
        {/* Imagem de fundo / preview */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={imgUrl}
          alt={label}
          className="absolute inset-0 w-full h-full object-cover transition-transform duration-300 group-hover:scale-105"
          loading="lazy"
        />

        {/* Overlay suave com gradiente */}
        <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-black/10 to-black/30 pointer-events-none" />

        {/* Header com badge de época */}
        <div className="relative z-10 p-2 flex items-center justify-between">
          <span className="inline-flex items-center px-1.5 py-0.5 rounded bg-black/60 border border-white/20 text-[10px] font-mono font-medium text-indigo-300 backdrop-blur-md">
            {label}
          </span>
          <button
            type="button"
            onClick={() => setSelectedSample(art)}
            title="Ampliar amostra"
            className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded-lg bg-black/60 hover:bg-black/80 text-zinc-200 border border-white/20 backdrop-blur-md"
          >
            <IconZoomIn className="size-3.5" />
          </button>
        </div>

        {/* Rodapé com tamanho e botão de download */}
        <div className="relative z-10 p-2 flex items-center justify-between text-[10px] font-mono">
          <span className="text-zinc-400">{formatBytes(art.bytes)}</span>
          {onDownload && (
            <button
              type="button"
              onClick={() => onDownload(jobId, art)}
              title="Baixar imagem"
              className="opacity-0 group-hover:opacity-100 transition-opacity p-1 rounded bg-black/60 hover:bg-black/80 text-zinc-200 border border-white/20 backdrop-blur-md"
            >
              <IconDownload className="size-3" />
            </button>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className={`space-y-2 ${className}`}>
      <div className="flex items-center justify-between">
        <span className="text-[10px] font-mono text-zinc-400 uppercase tracking-caps flex items-center gap-1.5">
          <IconImage className="size-3.5 text-indigo-400" />
          Amostras de Validação ({total})
        </span>
        <span className="text-[10px] font-mono text-zinc-400">
          LoRA Diffusion Previews
        </span>
      </div>

      {baselines.length > 0 && (
        <div className="space-y-1.5">
          <span className="block text-[10px] font-mono text-zinc-500 uppercase tracking-caps">
            Baseline (pré-treino)
          </span>
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
            {baselines.map(renderSampleCard)}
          </div>
        </div>
      )}

      {(visibleEpochSamples.length > 0 || untaggedSamples.length > 0) && (
        <div className="space-y-1.5">
          {epochSamples.length > 0 && (
            <span className="block text-[10px] font-mono text-zinc-500 uppercase tracking-caps">
              Amostras por época ({epochSamples.length})
            </span>
          )}
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
            {[...visibleEpochSamples, ...untaggedSamples].map(renderSampleCard)}
          </div>
        </div>
      )}

      {canCollapse && (
        <button
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[11px] font-mono text-zinc-300 transition hover:border-indigo-500/40 hover:bg-white/[0.06] hover:text-white cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
          aria-expanded={expanded}
        >
          {expanded ? (
            <IconChevronDown className="size-3" />
          ) : (
            <IconChevronRight className="size-3" />
          )}
          {expanded ? "Recolher" : `Mostrar todas (${total})`}
        </button>
      )}

      {/* Lightbox Modal para ampliação de alta resolução */}
      {selectedSample && (
        <div
          role="dialog"
          aria-modal="true"
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-md animate-fadeIn"
          onClick={() => setSelectedSample(null)}
        >
          <div
            className="relative max-w-2xl w-full bg-zinc-900/95 border border-white/15 rounded-2xl overflow-hidden shadow-2xl space-y-3 p-4 backdrop-blur-xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-white/10 pb-3">
              <div className="flex items-center gap-2">
                <span className="font-mono text-sm font-semibold text-zinc-100">
                  {formatSampleLabel(selectedSample.path)}
                </span>
                <span className="font-mono text-[11px] text-zinc-400">
                  · {selectedSample.path.split("/").pop()} · {formatBytes(selectedSample.bytes)}
                </span>
              </div>
              <button
                type="button"
                onClick={() => setSelectedSample(null)}
                className="p-1 rounded-lg text-zinc-400 hover:text-zinc-100 hover:bg-white/10 transition"
              >
                <IconX className="size-4" />
              </button>
            </div>

            <div className="flex justify-center bg-black/60 rounded-xl overflow-hidden max-h-[70vh] border border-white/5">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={`/api/jobs/${jobId}/artifacts/${selectedSample.id}/data`}
                alt={selectedSample.path}
                className="max-h-[70vh] w-auto object-contain"
              />
            </div>

            <div className="flex items-center justify-between pt-2">
              <span className="text-[11px] font-mono text-zinc-400">
                Amostra sintetizada por Diffusers LoRA
              </span>
              {onDownload && (
                <button
                  type="button"
                  onClick={() => onDownload(jobId, selectedSample)}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white font-mono text-xs font-medium transition cursor-pointer shadow-sm"
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
