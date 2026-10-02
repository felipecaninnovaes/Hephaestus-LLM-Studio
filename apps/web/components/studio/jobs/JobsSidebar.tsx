"use client";

import Link from "next/link";
import { IconActivity } from "@/components/icons";
import { Checkbox } from "@/components/ui/Checkbox";
import { JobListItem } from "@/components/studio/JobCard";
import type { Job, JobAlert } from "@/types/studio";
import { JobAlertsBadge } from "./JobAlertsPanel";

export const COMPARE_MAX_JOBS = 4;
export const COMPARE_MIN_JOBS = 2;

interface JobsSidebarProps {
  activeJobs: Job[];
  terminalJobs: Job[];
  selectedJobId: string | null;
  loading: boolean;
  totalCount: number;
  onSelectJob: (jobId: string) => void;
  onRerunJob: (job: Job) => void;
  /** Alertas carregados só do job selecionado (fatia 3c-UI — evita N requests por card). */
  selectedJobAlerts?: JobAlert[] | null;
  onOpenSelectedAlerts?: () => void;
  /** Seleção múltipla para comparação de runs (fatia 4c). */
  compareIds?: Set<string>;
  onToggleCompare?: (jobId: string) => void;
}

export function JobsSidebar({
  activeJobs,
  terminalJobs,
  selectedJobId,
  loading,
  totalCount,
  onSelectJob,
  onRerunJob,
  selectedJobAlerts,
  onOpenSelectedAlerts,
  compareIds,
  onToggleCompare,
}: JobsSidebarProps) {
  const compareFull = (compareIds?.size ?? 0) >= COMPARE_MAX_JOBS;

  function renderCompareCheckbox(job: Job) {
    if (!onToggleCompare || !compareIds) return null;
    const checked = compareIds.has(job.id);
    return (
      <Checkbox
        checked={checked}
        disabled={!checked && compareFull}
        onCheckedChange={() => onToggleCompare(job.id)}
        label={<span className="sr-only">Selecionar {job.model} para comparação</span>}
        className="relative z-10 shrink-0 pt-0"
      />
    );
  }

  function renderAlertsBadge(job: Job) {
    if (selectedJobId !== job.id || !onOpenSelectedAlerts) return null;
    return (
      <JobAlertsBadge alerts={selectedJobAlerts} onOpen={onOpenSelectedAlerts} />
    );
  }

  return (
    <aside className="w-full md:w-80 lg:w-96 shrink-0 md:sticky md:top-4 md:max-h-[calc(100vh-2rem)] md:overflow-y-auto overflow-x-hidden [scrollbar-width:thin] space-y-4">
      {loading ? (
        <div className="glass-card rounded-2xl p-8 text-center text-xs text-zinc-400 font-mono border border-white/10">
          Carregando execuções…
        </div>
      ) : totalCount === 0 ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-8 text-center border border-white/10">
          <span className="flex size-10 items-center justify-center rounded-xl border border-white/10 bg-white/5 text-zinc-400 backdrop-blur-sm">
            <IconActivity className="size-5 text-brand-400/60" />
          </span>
          <div className="space-y-1">
            <p className="text-xs font-semibold text-zinc-200">
              Nenhuma execução registrada
            </p>
            <p className="text-2xs text-zinc-400">
              Inicie um treino em{" "}
              <Link
                href="/treino"
                className="text-brand-400 hover:text-brand-300 underline underline-offset-2"
              >
                Treino YOLO
              </Link>{" "}
              ou aguarde jobs do AutoTracker.
            </p>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          {/* ── Jobs Ativos ── */}
          {activeJobs.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center justify-between px-1">
                <h3 className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
                  Em Execução
                </h3>
                <span className="flex items-center gap-1.5 font-mono text-2xs text-brand-300">
                  <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
                  {activeJobs.length}
                </span>
              </div>
              <div className="space-y-2">
                {activeJobs.map((job) => (
                  <JobListItem
                    key={job.id}
                    job={job}
                    isFocused={selectedJobId === job.id}
                    onSelect={onSelectJob}
                    leadingCheckbox={renderCompareCheckbox(job)}
                    alertsBadge={renderAlertsBadge(job)}
                  />
                ))}
              </div>
            </div>
          )}

          {/* ── Jobs Terminais ── */}
          {terminalJobs.length > 0 && (
            <div className="space-y-2">
              <div className="flex items-center justify-between px-1">
                <h3 className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
                  Histórico
                </h3>
                <span className="font-mono text-2xs text-zinc-500">
                  {terminalJobs.length}
                </span>
              </div>
              <div className="space-y-2">
                {terminalJobs.map((job) => (
                  <JobListItem
                    key={job.id}
                    job={job}
                    isFocused={selectedJobId === job.id}
                    onSelect={onSelectJob}
                    onRerun={onRerunJob}
                    leadingCheckbox={renderCompareCheckbox(job)}
                    alertsBadge={renderAlertsBadge(job)}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </aside>
  );
}
