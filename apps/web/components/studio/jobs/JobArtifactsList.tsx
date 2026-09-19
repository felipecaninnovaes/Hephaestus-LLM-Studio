"use client";

import { IconDownload, IconSparkles } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { formatBytes } from "@/lib/format";
import type { Job, JobArtifact } from "@/types/studio";

interface JobArtifactsListProps {
  job: Job;
  artifacts: JobArtifact[];
  onDownload: (jobId: string, art: JobArtifact) => void;
  onResume: (job: Job, art: JobArtifact) => void;
}

export function JobArtifactsList({
  job,
  artifacts,
  onDownload,
  onResume,
}: JobArtifactsListProps) {
  const filtered = artifacts.filter(
    (art) =>
      art.kind !== "sample" &&
      !art.path.startsWith("samples/") &&
      !art.path.includes("sample_epoch_"),
  );

  if (filtered.length === 0) return null;

  return (
    <div className="space-y-2">
      <h3 className="font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-300">
        Artefatos ({filtered.length})
      </h3>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2.5">
        {filtered.map((art) => (
          <div
            key={art.id}
            className="flex items-center justify-between rounded-xl border border-white/10 bg-white/[0.02] p-3 backdrop-blur-sm"
          >
            <div className="min-w-0 mr-2">
              <span
                className="block text-xs font-semibold text-zinc-200 truncate"
                title={art.path}
              >
                {art.path.split("/").pop()}
              </span>
              <span className="block font-mono text-2xs text-zinc-400">
                {formatBytes(art.bytes)} · {art.kind}
              </span>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              {(art.kind === "checkpoint" ||
                art.kind === "model" ||
                art.path.endsWith(".safetensors")) && (
                <Button
                  type="button"
                  variant="secondary"
                  size="sm"
                  onClick={() => onResume(job, art)}
                  title="Retomar treino a partir deste checkpoint"
                >
                  <IconSparkles className="size-3.5 text-sky-400" />
                  <span>Retomar</span>
                </Button>
              )}
              <Button
                type="button"
                variant="secondary"
                size="sm"
                onClick={() => onDownload(job.id, art)}
              >
                <IconDownload className="size-3.5" />
                <span>Baixar</span>
              </Button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
