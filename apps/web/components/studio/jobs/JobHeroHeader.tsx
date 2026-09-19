"use client";

import { IconActivity, IconTrash, IconX } from "@/components/icons";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import type { Job, JobStatus } from "@/types/studio";

const STATUS_LABEL: Record<JobStatus, string> = {
  preparing: "Preparando",
  queued: "Na fila",
  dispatched: "Despachando",
  running: "Executando",
  cancelling: "Cancelando",
  done: "Concluído",
  failed: "Falhou",
  cancelled: "Cancelado",
};

interface JobHeroHeaderProps {
  selectedJob: Job;
  focusMode: boolean;
  isActiveJob: boolean;
  selectedJobId: string | null;
  hasActiveJobs: boolean;
  activeJobId?: string;
  onSetFocus: (enable: boolean, jobId?: string) => void;
  onDelete: (job: Job) => void;
  onSelectJob: (jobId: string | null) => void;
}

export function JobHeroHeader({
  selectedJob,
  focusMode,
  isActiveJob,
  selectedJobId,
  hasActiveJobs,
  activeJobId,
  onSetFocus,
  onDelete,
  onSelectJob,
}: JobHeroHeaderProps) {
  return (
    <div className="flex items-center justify-between">
      <div className="flex items-center gap-2">
        <h2 className="font-display text-sm font-semibold text-zinc-200">
          {focusMode
            ? "Acompanhando"
            : isActiveJob
              ? "Execução Ativa"
              : "Detalhes da Execução"}
        </h2>
        <Badge
          variant={jobStatusToBadgeVariant(selectedJob.status)}
          pulse={selectedJob.status === "running"}
        >
          {STATUS_LABEL[selectedJob.status]}
        </Badge>
      </div>
      <div className="flex items-center gap-3">
        {focusMode ? (
          <button
            type="button"
            onClick={() => onSetFocus(false)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 min-h-[40px] text-2xs font-mono text-zinc-300 transition hover:bg-white/[0.06] active:scale-[0.985] cursor-pointer"
            title="Sair do modo foco"
          >
            <IconX className="size-3" />
            <span>Sair do foco</span>
          </button>
        ) : (
          <button
            type="button"
            onClick={() => onSetFocus(true, selectedJob.id)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 min-h-[40px] text-2xs font-mono text-zinc-300 transition hover:bg-white/[0.06] active:scale-[0.985] cursor-pointer"
            title="Acompanhar este job em tela cheia"
          >
            <IconActivity className="size-3" />
            <span>Acompanhar</span>
          </button>
        )}
        {!isActiveJob && (
          <button
            type="button"
            onClick={() => onDelete(selectedJob)}
            className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 min-h-[40px] text-2xs font-mono text-rose-300 transition hover:border-rose-500/40 hover:bg-rose-500/10 active:scale-[0.985] cursor-pointer"
            aria-label="Excluir job"
            title="Excluir este job e seus artefatos"
          >
            <IconTrash className="size-3" />
            <span>Excluir</span>
          </button>
        )}
        {selectedJobId && selectedJobId !== activeJobId && (
          <button
            type="button"
            onClick={() => onSelectJob(null)}
            className="text-xs text-zinc-400 hover:text-zinc-200 transition underline underline-offset-2"
          >
            {hasActiveJobs ? "Voltar ao job ativo" : "Ver último job"}
          </button>
        )}
      </div>
    </div>
  );
}
