"use client";

import { IconDownload, IconZoomIn } from "@/components/icons";
import { formatBytes } from "@/lib/format";
import { formatSampleLabel } from "@/lib/sampleArtifacts";
import type { JobArtifact } from "@/types/jobs";

interface SampleCardProps {
  jobId: string;
  art: JobArtifact;
  onZoom: (art: JobArtifact) => void;
  onDownload?: (jobId: string, art: JobArtifact) => void;
}

export function SampleCard({ jobId, art, onZoom, onDownload }: SampleCardProps) {
  const imgUrl = `/api/jobs/${jobId}/artifacts/${art.id}/data`;
  const label = formatSampleLabel(art.path);

  return (
    <div className="group relative aspect-square flex flex-col justify-between overflow-hidden rounded-xl border border-white/10 bg-black/40 transition-all hover:border-indigo-500/50 hover:shadow-lg hover:shadow-indigo-500/10">
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={imgUrl}
        alt={label}
        className="absolute inset-0 h-full w-full object-cover transition-transform duration-300 group-hover:scale-105"
        loading="lazy"
      />

      <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/80 via-black/10 to-black/30" />

      <div className="relative z-10 flex items-center justify-between p-2">
        <span className="inline-flex items-center rounded border border-white/20 bg-black/60 px-1.5 py-0.5 font-mono text-3xs font-medium text-indigo-300 backdrop-blur-md">
          {label}
        </span>
        <button
          type="button"
          onClick={() => onZoom(art)}
          title="Ampliar amostra"
          className="min-h-8 min-w-8 rounded-lg border border-white/20 bg-black/60 p-1 text-zinc-200 opacity-0 backdrop-blur-md transition-opacity hover:bg-black/80 group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
        >
          <IconZoomIn className="size-3.5" />
        </button>
      </div>

      <div className="relative z-10 flex items-center justify-between p-2 font-mono text-3xs">
        <span className="text-zinc-400">{formatBytes(art.bytes)}</span>
        {onDownload && (
          <button
            type="button"
            onClick={() => onDownload(jobId, art)}
            title="Baixar imagem"
            className="min-h-8 min-w-8 rounded border border-white/20 bg-black/60 p-1 text-zinc-200 opacity-0 backdrop-blur-md transition-opacity hover:bg-black/80 group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
          >
            <IconDownload className="size-3" />
          </button>
        )}
      </div>
    </div>
  );
}
