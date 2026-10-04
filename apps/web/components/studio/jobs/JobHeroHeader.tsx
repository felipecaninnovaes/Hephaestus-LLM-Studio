"use client";

import { IconActivity, IconTrash, IconX } from "@/components/icons";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import type { Job, JobAlert, JobStatus } from "@/types/studio";
import { JobAlertsBadge } from "./JobAlertsPanel";

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
  alerts?: JobAlert[] | null;
  onOpenAlerts?: () => void;
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
  alerts,
  onOpenAlerts,
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
        {onOpenAlerts && (
          <JobAlertsBadge alerts={alerts} onOpen={onOpenAlerts} />
        )}
      </div>
      <div className="flex items-center gap-3">
        {focusMode ? (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onSetFocus(false)}
            leftIcon={<IconX className="size-3.5" />}
            title="Sair do modo foco"
          >
            <span>Sair do foco</span>
          </Button>
        ) : (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onSetFocus(true, selectedJob.id)}
            leftIcon={<IconActivity className="size-3.5" />}
            title="Acompanhar este job em tela cheia"
          >
            <span>Acompanhar</span>
          </Button>
        )}
        {!isActiveJob && (
          <Button
            type="button"
            variant="destructive"
            size="sm"
            onClick={() => onDelete(selectedJob)}
            leftIcon={<IconTrash className="size-3.5" />}
            aria-label="Excluir job"
            title="Excluir este job e seus artefatos"
          >
            <span>Excluir</span>
          </Button>
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
