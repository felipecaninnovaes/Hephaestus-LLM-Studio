"use client";

import React from "react";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import {
  IconCheck,
  IconChevronDown,
  IconDownload,
  IconRefresh,
  IconTarget,
  IconTrash,
  IconX,
  IconSparkles,
} from "@/components/icons";
import { formatBytes, formatDuration, formatRelativeTime } from "@/lib/format";
import type { Job, JobArtifact, JobMetrics, JobStatus } from "@/types/studio";

export const JOB_STATUS_CONFIG: Record<
  JobStatus,
  {
    borderClass: string;
    badgeClass: string;
    iconBg: string;
    iconColor: string;
    label: string;
  }
> = {
  queued: {
    borderClass: "bg-amber-500",
    badgeClass: "bg-amber-500/15 text-amber-300 border-amber-500/30 backdrop-blur-sm",
    iconBg: "bg-amber-500/15 backdrop-blur-sm",
    iconColor: "text-amber-400",
    label: "Na fila",
  },
  running: {
    borderClass: "bg-brand-500",
    badgeClass: "bg-brand-500/15 text-brand-300 border-brand-500/35 backdrop-blur-sm",
    iconBg: "bg-brand-500/15 backdrop-blur-sm",
    iconColor: "text-brand-400",
    label: "Executando",
  },
  cancelling: {
    borderClass: "bg-amber-500",
    badgeClass: "bg-amber-500/15 text-amber-300 border-amber-500/30 backdrop-blur-sm",
    iconBg: "bg-amber-500/15 backdrop-blur-sm",
    iconColor: "text-amber-400",
    label: "Cancelando",
  },
  done: {
    borderClass: "bg-[#34d399]",
    badgeClass: "bg-[#34d399]/15 text-[#34d399] border-[#34d399]/30 backdrop-blur-sm",
    iconBg: "bg-[#34d399]/15 backdrop-blur-sm",
    iconColor: "text-[#34d399]",
    label: "Concluído",
  },
  failed: {
    borderClass: "bg-rose-500",
    badgeClass: "bg-rose-500/15 text-rose-300 border-rose-500/30 backdrop-blur-sm",
    iconBg: "bg-rose-500/15 backdrop-blur-sm",
    iconColor: "text-rose-400",
    label: "Falhou",
  },
  cancelled: {
    borderClass: "bg-zinc-600",
    badgeClass: "bg-zinc-800 text-zinc-400 border-zinc-700 backdrop-blur-sm",
    iconBg: "bg-zinc-800 backdrop-blur-sm",
    iconColor: "text-zinc-400",
    label: "Cancelado",
  },
};

export interface JobArtifactsListProps {
  jobId: string;
  artifacts: JobArtifact[];
  onDownload: (jobId: string, art: JobArtifact) => void;
  onResume?: (jobId: string, art: JobArtifact) => void;
  className?: string;
}

export function JobArtifactsList({
  jobId,
  artifacts,
  onDownload,
  onResume,
  className = "",
}: JobArtifactsListProps) {
  if (artifacts.length === 0) return null;

  return (
    <div className={`space-y-1.5 ${className}`}>
      <div className="text-[10px] font-mono text-zinc-400 uppercase tracking-caps mb-1.5">
        Artefatos Gerados ({artifacts.length})
      </div>
      <div className="space-y-1.5">
        {artifacts.map((art) => (
          <div
            key={art.id}
            className="flex items-center justify-between rounded-lg border border-white/10 bg-white/[0.03] backdrop-blur-sm px-3 py-1.5 text-[11px]"
          >
            <span
              className="truncate text-zinc-300 mr-2 font-mono text-[11px]"
              title={art.path}
            >
              {art.path.split("/").pop()} ({formatBytes(art.bytes)})
            </span>
            <div className="flex items-center gap-2.5 shrink-0">
              {onResume &&
                (art.kind === "checkpoint" ||
                  art.kind === "model" ||
                  art.path.endsWith(".safetensors")) && (
                  <button
                    type="button"
                    onClick={() => onResume(jobId, art)}
                    className="inline-flex items-center gap-1 text-sky-400 hover:text-sky-300 font-mono text-[11px] font-medium cursor-pointer"
                    title="Retomar treino a partir deste checkpoint"
                  >
                    <IconSparkles className="size-3" />
                    <span>Retomar</span>
                  </button>
                )}
              <button
                type="button"
                onClick={() => onDownload(jobId, art)}
                className="inline-flex items-center gap-1 text-brand-400 hover:text-brand-300 font-mono text-[11px] font-medium cursor-pointer"
              >
                <IconDownload className="size-3" />
                <span>Baixar</span>
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export interface JobListItemProps {
  job: Job;
  isFocused?: boolean;
  onSelect?: (jobId: string) => void;
  actionButton?: React.ReactNode;
}

export function JobListItem({
  job,
  isFocused = false,
  onSelect,
  actionButton,
}: JobListItemProps) {
  const config = JOB_STATUS_CONFIG[job.status] || JOB_STATUS_CONFIG.queued;
  const isActive =
    job.status === "queued" ||
    job.status === "running" ||
    job.status === "cancelling";
  const pct = Math.round((job.progress ?? 0) * 100);

  return (
    <div
      onClick={() => onSelect?.(job.id)}
      role={onSelect ? "button" : undefined}
      tabIndex={onSelect ? 0 : undefined}
      onKeyDown={(e) => {
        if (onSelect && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          onSelect(job.id);
        }
      }}
      className={`glass-card group relative flex flex-col sm:flex-row sm:items-center justify-between gap-3.5 rounded-xl p-4 transition-all duration-200 border ${
        onSelect ? "cursor-pointer" : ""
      } ${
        isFocused
          ? "border-brand-500/50 bg-brand-500/[0.12] shadow-lg shadow-brand-500/10 ring-1 ring-brand-500/30"
          : "border-white/10 hover:border-brand-500/30 hover:bg-white/[0.04]"
      }`}
    >
      {/* Indicador de foco lateral óptico */}
      <div
        className={`absolute left-0 inset-y-2.5 w-1 rounded-r-full bg-brand-500 transition-opacity ${
          isFocused ? "opacity-100" : "opacity-0 group-hover:opacity-40"
        }`}
      />

      <div className="flex items-center gap-3.5 min-w-0 flex-1 pl-1">
        <Badge
          variant={jobStatusToBadgeVariant(job.status)}
          pulse={job.status === "running"}
          className="shrink-0"
        >
          {config.label}
        </Badge>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 min-w-0">
            <span className="text-xs font-semibold text-zinc-100 shrink-0">
              {job.model}
            </span>
            <span
              className="font-mono text-[11px] text-zinc-400 truncate"
              title={`${job.kind} · ${job.engine}`}
            >
              · {job.kind} · {job.engine}
            </span>
            {isFocused && (
              <span className="hidden sm:inline-flex shrink-0 items-center rounded-md border border-brand-500/30 bg-brand-500/15 px-2 py-0.5 font-mono text-[11px] font-medium text-brand-300 backdrop-blur-sm">
                Ativo no monitor
              </span>
            )}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-3 font-mono text-[11px] text-zinc-400">
            <span>{formatRelativeTime(job.createdAt)}</span>
            <span>Duração: {formatDuration(job.createdAt, job.finishedAt)}</span>
            {isActive && (
              <span className="text-brand-300 font-semibold">{pct}%</span>
            )}
            {job.phase && (
              <span className="text-violet-300 font-medium bg-violet-500/10 px-1.5 py-0.2 rounded border border-violet-500/20">
                {job.phase}
              </span>
            )}
            {job.vramUsedGb ? (
              <span className="text-zinc-300">
                {job.vramUsedGb.toFixed(1)} GB VRAM
              </span>
            ) : null}
            {job.orchestratorName ? (
              <span className="flex items-center gap-1.5 text-zinc-300">
                <span className="text-zinc-600">·</span>
                <span>
                  {job.orchestratorName}
                  {job.orchestratorKind ? ` (${job.orchestratorKind})` : ""}
                </span>
                {job.orchestratorFallback && (
                  <span
                    className="inline-flex items-center rounded border border-amber-500/30 bg-amber-500/10 px-1 py-0.2 text-[9px] text-amber-400 font-medium"
                    title="Job sofreu fallback automático após timeout no nó solicitado"
                  >
                    fallback
                  </span>
                )}
              </span>
            ) : null}
          </div>
        </div>
      </div>

      {actionButton && (
        <div className="flex items-center space-x-2 shrink-0 pl-1 sm:pl-0">
          {actionButton}
        </div>
      )}
    </div>
  );
}

export default JobListItem;
