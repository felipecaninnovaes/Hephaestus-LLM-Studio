"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  IconBell,
  IconRefresh,
  IconTarget,
  IconTrash,
} from "@/components/icons";
import { AutolabelReviewModal } from "@/components/studio/AutolabelReviewModal";
import { AutotrackerReviewModal } from "@/components/studio/AutotrackerReviewModal";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { JobCleanupDialog } from "@/components/studio/JobCleanupDialog";
import {
  Button,
  Drawer,
  SearchInput,
  SubmodulePills,
} from "@/components/ui";
import { useJobLifecycle } from "@/hooks/useJobLifecycle";
import {
  getJobArtifacts,
  getJobMetrics,
  getTelemetry,
  listJobs,
} from "@/lib/jobs";
import {
  buildDiffusionRerun,
  buildDiffusionResume,
  buildYoloRerun,
} from "@/lib/paramsToPreset";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  Telemetry,
} from "@/types/studio";
import { JOB_STATUS_CONFIG } from "../JobCard";
import { ActionCenterEmptyState } from "./ActionCenterEmptyState";
import { ActionCenterJobItem } from "./ActionCenterJobItem";
import { ActionCenterNotificationItem } from "./ActionCenterNotificationItem";
import { useSystemNotifications } from "./useSystemNotifications";

export interface ActionCenterProps {
  open: boolean;
  onClose: () => void;
}

export type TabFilter = "all" | "active" | "jobs" | "system";

const STATUS_CONFIG = JOB_STATUS_CONFIG;

export function ActionCenter({ open, onClose }: ActionCenterProps) {
  const router = useRouter();
  const [jobs, setJobs] = useState<Job[]>([]);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<TabFilter>("all");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  // Detalhes sob demanda (métricas e artefatos por jobId)
  const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
  const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
  const [reviewJob, setReviewJob] = useState<Job | null>(null);
  const [autotrackerReviewJob, setAutotrackerReviewJob] = useState<Job | null>(
    null,
  );
  const [cleanupOpen, setCleanupOpen] = useState(false);

  const {
    abortTarget,
    setAbortTarget,
    abortBusy,
    handleAbort,
    deleteTarget,
    setDeleteTarget,
    deleteBusy,
    handleDeleteJob,
    applyBusy,
    applyOverwrite,
    setApplyOverwrite,
    handleApplyBoxes,
    handleApplyCaptions,
    handleDownloadArtifact,
  } = useJobLifecycle({
    onSuccess: () => void fetchData(),
    onDeleted: () => void fetchData(),
    onNavigateDataset: (datasetId) => {
      onClose();
      router.push(`/datasets/${datasetId}`);
    },
  });

  const pollRef = useRef<number | null>(null);

  // Busca lista de jobs e telemetria do nó em paralelo
  const fetchData = useCallback(async () => {
    try {
      setLoading(true);
      const [resJobs, resTel] = await Promise.allSettled([
        listJobs(),
        getTelemetry(),
      ]);

      if (resJobs.status === "fulfilled") {
        setJobs(resJobs.value.items || []);
      }
      if (resTel.status === "fulfilled") {
        setTelemetry(resTel.value);
      }
    } catch {
      // Ignora erro silenciosamente em polling
    } finally {
      setLoading(false);
    }
  }, []);

  // Polling de atividades consciente de visibilidade (apenas com drawer aberto)
  useEffect(() => {
    if (!open) {
      if (pollRef.current !== null) {
        window.clearInterval(pollRef.current);
        pollRef.current = null;
      }
      return;
    }

    // Busca inicial imediata ao abrir o drawer
    void fetchData();

    function tick() {
      if (document.visibilityState === "visible") {
        void fetchData();
      }
    }

    pollRef.current = window.setInterval(tick, 3000);

    function onVisibilityChange() {
      if (document.visibilityState === "visible") {
        void fetchData();
        if (pollRef.current === null) {
          pollRef.current = window.setInterval(tick, 3000);
        }
      } else if (pollRef.current !== null) {
        window.clearInterval(pollRef.current);
        pollRef.current = null;
      }
    }

    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [open, fetchData]);

  // Carrega métricas e artefatos ao expandir um job
  useEffect(() => {
    if (!expandedId) return;
    const currentId = expandedId;

    async function loadJobExtra() {
      try {
        const [m, a] = await Promise.all([
          getJobMetrics(currentId),
          getJobArtifacts(currentId),
        ]);
        setMetrics((prev) => ({ ...prev, [currentId]: m.items }));
        setArtifacts((prev) => ({ ...prev, [currentId]: a.items }));
      } catch {
        // Detalhe best-effort
      }
    }

    void loadJobExtra();
  }, [expandedId]);

  function handleResume(job: Job, art: JobArtifact) {
    const resumeData = buildDiffusionResume(job, art);

    try {
      sessionStorage.setItem(
        "hephaestus_diffusion_resume",
        JSON.stringify(resumeData),
      );
    } catch {
      // Best-effort
    }

    onClose();
    router.push("/difusao");
  }

  function handleRerun(job: Job) {
    if (job.engine === "diffusion") {
      const resumeData = buildDiffusionRerun(job);

      try {
        sessionStorage.setItem(
          "hephaestus_diffusion_resume",
          JSON.stringify(resumeData),
        );
      } catch {
        // Best-effort
      }

      onClose();
      router.push("/difusao");
      return;
    }

    if (job.engine === "yolo") {
      const rerunParams = buildYoloRerun(job);

      try {
        sessionStorage.setItem("heph_rerun_yolo", JSON.stringify(rerunParams));
      } catch {
        // Best-effort
      }

      onClose();
      router.push("/treino");
    }
  }

  // Notificações do sistema agnósticas (telemetria, nó orquestrador, alertas de recursos)
  const systemNotifications = useSystemNotifications(telemetry, jobs);

  // Filtragem e ordenação de jobs (mais recentes primeiro)
  const sortedJobs = useMemo(() => {
    return [...jobs].sort(
      (a, b) =>
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    );
  }, [jobs]);

  const activeJobsCount = useMemo(
    () =>
      jobs.filter(
        (j) =>
          j.status === "running" ||
          j.status === "preparing" ||
          j.status === "queued" ||
          j.status === "dispatched" ||
          j.status === "cancelling",
      ).length,
    [jobs],
  );

  const activeAlertsCount = useMemo(
    () =>
      systemNotifications.filter(
        (n) => n.level === "warning" || n.level === "error",
      ).length,
    [systemNotifications],
  );

  const totalActiveCount = activeJobsCount + activeAlertsCount;

  // Filtragem por aba e busca unificada
  const q = query.trim().toLowerCase();

  const filteredJobs = useMemo(() => {
    if (tab === "system") return [];

    let list = sortedJobs;
    if (tab === "active") {
      list = list.filter(
        (j) =>
          j.status === "running" ||
          j.status === "preparing" ||
          j.status === "queued" ||
          j.status === "dispatched" ||
          j.status === "cancelling",
      );
    }

    if (!q) return list;

    return list.filter((j) => {
      const modelMatch = j.model.toLowerCase().includes(q);
      const kindMatch = j.kind.toLowerCase().includes(q);
      const idMatch = j.id.toLowerCase().includes(q);
      const engineMatch = j.engine?.toLowerCase().includes(q);
      const statusLabel = STATUS_CONFIG[j.status]?.label.toLowerCase() || "";
      return (
        modelMatch ||
        kindMatch ||
        idMatch ||
        engineMatch ||
        statusLabel.includes(q)
      );
    });
  }, [sortedJobs, tab, q]);

  const filteredNotifications = useMemo(() => {
    if (tab === "jobs") return [];

    let list = systemNotifications;
    if (tab === "active") {
      list = list.filter((n) => n.level === "warning" || n.level === "error");
    }

    if (!q) return list;

    return list.filter((n) => {
      const titleMatch = n.title.toLowerCase().includes(q);
      const messageMatch = n.message.toLowerCase().includes(q);
      const categoryMatch = n.category.toLowerCase().includes(q);
      return titleMatch || messageMatch || categoryMatch;
    });
  }, [systemNotifications, tab, q]);

  function toggleExpand(id: string) {
    setExpandedId((prev) => (prev === id ? null : id));
  }

  return (
    <>
      <Drawer
        open={open}
        onClose={onClose}
        title="Centro de Atividades"
        icon={<IconBell className="size-4" />}
        ariaLabel="Centro de Atividades"
        widthClass="w-full sm:w-[520px]"
        headerRight={
          <div className="flex items-center space-x-1.5">
            {totalActiveCount > 0 && (
              <span className="mr-1 flex items-center space-x-1.5 rounded-full border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm px-2.5 py-0.5 font-mono text-2xs font-medium text-brand-300">
                <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
                <span className="tabular-nums">
                  {totalActiveCount} ativo{totalActiveCount > 1 ? "s" : ""}
                </span>
              </span>
            )}
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setCleanupOpen(true)}
              title="Limpar jobs antigos"
              aria-label="Limpar jobs antigos"
            >
              <IconTrash className="size-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => void fetchData()}
              title="Atualizar atividades e telemetria"
              aria-label="Atualizar atividades"
              disabled={loading}
            >
              <IconRefresh
                className={`size-3.5 ${loading ? "animate-spin motion-reduce:animate-none text-brand-400" : ""}`}
              />
            </Button>
          </div>
        }
        footer={
          <div className="p-3.5 flex flex-col sm:flex-row items-center justify-between gap-3">
            <div className="flex items-center space-x-2 text-2xs font-mono text-zinc-400">
              <span
                className={`size-2 rounded-full ${telemetry ? "bg-status-success animate-pulse motion-reduce:animate-none" : "bg-zinc-500"}`}
              />
              <span>
                Nó Local:{" "}
                <strong className="text-zinc-200 font-semibold">
                  {telemetry
                    ? `${telemetry.jobsActive} ativo(s)${telemetry.vramUsed !== null ? ` · ${(telemetry.vramUsed / 1024).toFixed(1)} GB VRAM` : ""}${telemetry.cpu !== null ? ` · ${telemetry.cpu}% CPU` : ""}`
                    : "Sem telemetria"}
                </strong>
              </span>
            </div>
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => {
                  onClose();
                  router.push("/jobs");
                }}
                className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-brand-500/30 hover:bg-white/[0.08] hover:text-white cursor-pointer"
                title="Abrir Forja de Treino"
              >
                <IconTarget className="size-3.5 text-brand-400" />
                <span>Forja</span>
              </button>
            </div>
          </div>
        }
      >
        {/* Abas de Filtro Agnósticas */}
        <div className="border-b border-white/10 px-3 py-2 bg-black/30 backdrop-blur-sm">
          <SubmodulePills<TabFilter>
            size="sm"
            value={tab}
            onChange={setTab}
            items={[
              {
                id: "all",
                label: "Tudo",
                count: sortedJobs.length + systemNotifications.length,
              },
              { id: "active", label: "Em andamento", count: totalActiveCount },
              {
                id: "jobs",
                label: "Tarefas & Treino",
                count: sortedJobs.length,
              },
              {
                id: "system",
                label: "Sistema & Alertas",
                count: systemNotifications.length,
              },
            ]}
          />
        </div>

        {/* Barra de Pesquisa */}
        <div className="border-b border-white/10 p-3 bg-white/[0.01]">
          <SearchInput
            size="md"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onClear={() => setQuery("")}
            placeholder="Filtrar por modelo, ID, serviço ou status…"
          />
        </div>

        {/* Lista Unificada de Atividades, Notificações & Jobs */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3 [scrollbar-width:thin]">
          {filteredNotifications.length === 0 && filteredJobs.length === 0 ? (
            <ActionCenterEmptyState query={query} onClose={onClose} />
          ) : (
            <>
              {/* 1. Bloco de Notificações do Sistema */}
              {filteredNotifications.length > 0 && (
                <div className="space-y-2">
                  {tab === "all" && filteredJobs.length > 0 && (
                    <div className="flex items-center justify-between px-1">
                      <span className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-400">
                        Alertas do Sistema & Nó ({filteredNotifications.length})
                      </span>
                    </div>
                  )}
                  {filteredNotifications.map((notif) => (
                    <ActionCenterNotificationItem
                      key={notif.id}
                      notification={notif}
                      onClose={onClose}
                    />
                  ))}
                </div>
              )}

              {/* 2. Bloco de Tarefas & Treinamentos (Jobs) */}
              {filteredJobs.length > 0 && (
                <div className="space-y-2">
                  {tab === "all" && filteredNotifications.length > 0 && (
                    <div className="flex items-center justify-between px-1 pt-2">
                      <span className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-400">
                        Processamentos & Treinos ({filteredJobs.length})
                      </span>
                    </div>
                  )}
                  {filteredJobs.map((job) => (
                    <ActionCenterJobItem
                      key={job.id}
                      job={job}
                      isExpanded={expandedId === job.id}
                      onToggleExpand={toggleExpand}
                      metrics={metrics[job.id]}
                      artifacts={artifacts[job.id]}
                      onClose={onClose}
                      onDownload={handleDownloadArtifact}
                      onResume={handleResume}
                      onRerun={handleRerun}
                      onAbort={setAbortTarget}
                      onDelete={setDeleteTarget}
                      onReviewAutolabel={setReviewJob}
                      onReviewAutotracker={setAutotrackerReviewJob}
                      applyBusy={applyBusy}
                      applyOverwrite={applyOverwrite}
                      setApplyOverwrite={setApplyOverwrite}
                      onApplyBoxes={handleApplyBoxes}
                      onApplyCaptions={handleApplyCaptions}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </Drawer>

      {/* Confirmação de cancelamento de Job */}
      <ConfirmDialog
        open={Boolean(abortTarget)}
        title="Cancelar execução do Job"
        body={
          <p className="text-xs text-zinc-300">
            Tem certeza de que deseja cancelar a execução de{" "}
            <strong className="text-white font-mono">{abortTarget?.model}</strong>{" "}
            ({abortTarget?.id.slice(0, 8)}…)? O processo será interrompido no nó.
          </p>
        }
        confirmLabel="Cancelar Job"
        danger
        busy={abortBusy}
        onConfirm={handleAbort}
        onClose={() => setAbortTarget(null)}
      />

      {/* Confirmação de exclusão de Job */}
      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="Excluir job"
        body={
          <p className="text-xs text-zinc-300">
            Tem certeza de que deseja excluir o job{" "}
            <strong className="text-white font-mono">{deleteTarget?.model}</strong>{" "}
            ({deleteTarget?.id.slice(0, 8)}…)? Os artefatos temporários no S3 serão
            removidos. Modelos promovidos e gerações salvas são preservados.
          </p>
        }
        confirmLabel="Excluir"
        danger
        busy={deleteBusy}
        onConfirm={handleDeleteJob}
        onClose={() => setDeleteTarget(null)}
      />

      {/* Diálogo de limpeza em lote */}
      <JobCleanupDialog
        open={cleanupOpen}
        onClose={() => setCleanupOpen(false)}
        terminalJobs={jobs
          .filter(
            (j) =>
              j.status === "done" ||
              j.status === "failed" ||
              j.status === "cancelled",
          )
          .map((j) => ({
            id: j.id,
            status: j.status as "done" | "failed" | "cancelled",
            createdAt: j.createdAt,
            finishedAt: j.finishedAt,
          }))}
        onDone={() => void fetchData()}
      />

      {/* Modal de Revisão e Curadoria de Legendas (AutoLabel) */}
      <AutolabelReviewModal
        open={Boolean(reviewJob)}
        onClose={() => setReviewJob(null)}
        jobId={reviewJob?.id ?? null}
        datasetId={reviewJob?.datasetId}
        onApplied={() => {
          void fetchData();
        }}
      />

      {/* Modal de Revisão e Criação de Classes Ausentes (AutoTracker) */}
      {autotrackerReviewJob && (
        <AutotrackerReviewModal
          open={Boolean(autotrackerReviewJob)}
          job={autotrackerReviewJob}
          onClose={() => setAutotrackerReviewJob(null)}
          onApplied={() => {
            void fetchData();
          }}
        />
      )}
    </>
  );
}

export default ActionCenter;
