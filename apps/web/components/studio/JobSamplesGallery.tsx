"use client";

import React, { useState } from "react";
import { IconDownload, IconImage, IconX, IconZoomIn } from "@/components/icons";
import { formatBytes } from "@/lib/format";
import type { JobArtifact } from "@/types/studio";

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

  // Filtra apenas artefatos que são amostras geradas (kind === 'sample' ou path com 'samples/')
  const sampleArtifacts = artifacts.filter(
    (art) =>
      art.kind === "sample" ||
      art.path.startsWith("samples/") ||
      art.path.includes("sample_epoch_") ||
      (art.path.endsWith(".png") && art.kind !== "model" && art.kind !== "metrics"),
  );

  if (sampleArtifacts.length === 0) return null;

  // Helper para extrair o rótulo da época
  function formatSampleLabel(path: string): string {
    const match = path.match(/epoch_(\d+)/i);
    if (match) {
      return `Época ${parseInt(match[1], 10)}`;
    }
    const fname = path.split("/").pop() || path;
    return fname.replace(/\.[^/.]+$/, "");
  }

  return (
    <div className={`space-y-2 ${className}`}>
      <div className="flex items-center justify-between">
        <span className="text-[10px] font-mono text-zinc-400 uppercase tracking-caps flex items-center gap-1.5">
          <IconImage className="size-3.5 text-indigo-400" />
          Amostras de Validação ({sampleArtifacts.length})
        </span>
        <span className="text-[10px] font-mono text-zinc-400">
          LoRA Diffusion Previews
        </span>
      </div>

      <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
        {sampleArtifacts.map((art) => {
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
        })}
      </div>

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
