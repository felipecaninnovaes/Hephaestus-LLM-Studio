"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconCheck,
  IconChevronDown,
  IconDatabase,
  IconDownload,
  IconPlay,
  IconRefresh,
  IconTarget,
  IconTrash,
  IconX,
  IconZap,
} from "@/components/icons";
import { SearchInput, SubmodulePills, Badge, jobStatusToBadgeVariant } from "@/components/ui";
import {
  abortJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  listJobs,
} from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { ApiError } from "@/lib/api";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import { autotrackerErrorMessage } from "@/types/studio";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
} from "@/types/studio";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import { showToast } from "./Toast";

interface ActionCenterProps {
  open: boolean;
  onClose: () => void;
}

type TabFilter = "all" | "running" | "done" | "failed";

const STATUS_CONFIG: Record<
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

function formatDuration(start: string, end: string | null): string {
  const ms =
    (end ? new Date(end).getTime() : Date.now()) - new Date(start).getTime();
  if (ms < 0) return "—";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function ActionCenter({ open, onClose }: ActionCenterProps) {
  const router = useRouter();
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<TabFilter>("all");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);

  // Detalhes sob demanda (métricas e artefatos por jobId)
  const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
  const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
  const [abortTarget, setAbortTarget] = useState<Job | null>(null);
  const [abortBusy, setAbortBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [applyOverwrite, setApplyOverwrite] = useState(false);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Transição do drawer
  useEffect(() => {
    if (open) {
      setMounted(true);
      const raf = requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          setVisible(true);
        });
      });
      return () => cancelAnimationFrame(raf);
    } else {
      setVisible(false);
      const timer = setTimeout(() => {
        setMounted(false);
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [open]);

  // Tecla Escape fecha drawer
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Bloqueio do scroll do body
  useEffect(() => {
    if (mounted) {
      const original = document.body.style.overflow;
      document.body.style.overflow = "hidden";
      return () => {
        document.body.style.overflow = original;
      };
    }
  }, [mounted]);

  // Busca lista de jobs
  const fetchJobs = useCallback(async () => {
    try {
      setLoading(true);
      const res = await listJobs();
      setJobs(res.items);
    } catch {
      // Ignora erro silenciosamente em polling
    } finally {
      setLoading(false);
    }
  }, []);

  // Busca inicial ao abrir
  useEffect(() => {
    if (open) {
      fetchJobs();
    }
  }, [open, fetchJobs]);

  // Polling a cada 3s quando aberto ou quando há jobs ativos
  useEffect(() => {
    const hasActive = jobs.some(
      (j) =>
        j.status === "queued" ||
        j.status === "running" ||
        j.status === "cancelling",
    );

    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }

    if (open || hasActive) {
      pollRef.current = setInterval(() => {
        void fetchJobs();
      }, 3000);
    }

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [open, jobs, fetchJobs]);

  // Carrega métricas e artefatos ao expandir um job
  useEffect(() => {
    if (!expandedId) return;
    const ctrl = new AbortController();

    async function loadJobExtra() {
      try {
        const [m, a] = await Promise.all([
          getJobMetrics(expandedId!),
          getJobArtifacts(expandedId!),
        ]);
        if (!ctrl.signal.aborted) {
          setMetrics((prev) => ({ ...prev, [expandedId!]: m.items }));
          setArtifacts((prev) => ({ ...prev, [expandedId!]: a.items }));
        }
      } catch {
        // Detalhe best-effort
      }
    }

    loadJobExtra();
    return () => ctrl.abort();
  }, [expandedId]);

  // Ações de cancelamento de job
  async function handleAbort() {
    if (!abortTarget) return;
    setAbortBusy(true);
    try {
      await abortJob(abortTarget.id);
      showToast("Job cancelado com sucesso.", "success");
      setAbortTarget(null);
      await fetchJobs();
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "job_not_abortable" || err.status === 409)
      ) {
        showToast("Este job não pode mais ser cancelado.", "info");
        setAbortTarget(null);
        return;
      }
      showToast("Falha ao cancelar job.", "error");
    } finally {
      setAbortBusy(false);
    }
  }

  // Ação de aplicar boxes do AutoTracker
  async function handleApplyBoxes(job: Job) {
    setApplyBusy(true);
    try {
      const result = await applyAutotrackerBoxes(job.id, {
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} boxes aplicadas em ${result.images} imagens.`,
        "success",
        job.datasetId
          ? {
              label: "Abrir dataset",
              onClick: () => {
                onClose();
                router.push(`/datasets/${job.datasetId}`);
              },
            }
          : undefined,
      );
      setApplyOverwrite(false);
      await fetchJobs();
    } catch (err) {
      if (err instanceof ApiError) {
        showToast(autotrackerErrorMessage(err.code), "error");
        return;
      }
      showToast("Falha ao aplicar boxes ao dataset.", "error");
    } finally {
      setApplyBusy(false);
    }
  }

  // Ação de download de artefato
  async function handleDownload(jobId: string, art: JobArtifact) {
    try {
      const filename = art.path.split("/").pop() || "artefato.bin";
      await downloadArtifact(jobId, art.id, filename);
    } catch {
      showToast("Falha ao baixar artefato.", "error");
    }
  }

  // Filtragem e ordenação (mais recentes primeiro)
  const sortedJobs = useMemo(() => {
    return [...jobs].sort(
      (a, b) =>
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    );
  }, [jobs]);

  const activeCount = useMemo(
    () =>
      jobs.filter(
        (j) =>
          j.status === "running" ||
          j.status === "queued" ||
          j.status === "cancelling",
      ).length,
    [jobs],
  );

  const doneCount = useMemo(
    () => jobs.filter((j) => j.status === "done").length,
    [jobs],
  );

  const failedCount = useMemo(
    () =>
      jobs.filter((j) => j.status === "failed" || j.status === "cancelled")
        .length,
    [jobs],
  );

  const filtered = useMemo(() => {
    let list = sortedJobs;

    if (tab === "running") {
      list = list.filter(
        (j) =>
          j.status === "running" ||
          j.status === "queued" ||
          j.status === "cancelling",
      );
    } else if (tab === "done") {
      list = list.filter((j) => j.status === "done");
    } else if (tab === "failed") {
      list = list.filter(
        (j) => j.status === "failed" || j.status === "cancelled",
      );
    }

    const q = query.trim().toLowerCase();
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
  }, [sortedJobs, tab, query]);

  function toggleExpand(id: string) {
    setExpandedId((prev) => (prev === id ? null : id));
  }

  if (!mounted) return null;

  return (
    <>
      <div className="fixed inset-0 z-50 overflow-hidden pointer-events-none">
        {/* Backdrop com desfoque e fade-in fluido */}
        <div
          className={`fixed inset-0 bg-black/60 backdrop-blur-sm transition-opacity duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] pointer-events-auto ${
            visible ? "opacity-100" : "opacity-0"
          }`}
          onClick={onClose}
          aria-hidden="true"
        />

        {/* Drawer lateral direito */}
        <aside
          role="dialog"
          aria-modal="true"
          aria-label="Centro de Atividades"
          className={`fixed inset-y-0 right-0 flex w-full flex-col border-l border-white/10 bg-[rgba(18,15,24,0.85)] text-zinc-100 shadow-[-24px_0_60px_rgba(0,0,0,0.85)] backdrop-blur-2xl transition-transform duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] sm:w-[500px] pointer-events-auto ${
            visible ? "translate-x-0" : "translate-x-full"
          }`}
        >
          {/* Hairline zenital com gradiente violeta */}
          <span
            aria-hidden="true"
            className="pointer-events-none absolute top-0 left-0 right-0 h-px"
            style={{
              background:
                "linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)",
            }}
          />

          {/* Header */}
          <div className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-5">
            <div className="flex items-center space-x-2.5">
              <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400">
                <IconZap className="size-4" />
              </span>
              <div>
                <h2 className="font-display text-sm font-bold text-white tracking-tight">
                  Centro de Atividades
                </h2>
              </div>
              {activeCount > 0 && (
                <span className="ml-1.5 flex items-center space-x-1 rounded-full border border-brand-500/35 bg-brand-500/20 backdrop-blur-sm px-2 py-0.5 font-mono text-[10px] font-medium text-brand-300">
                  <span className="size-1.5 rounded-full bg-brand-400 animate-pulse" />
                  <span>{activeCount} ativo{activeCount > 1 ? "s" : ""}</span>
                </span>
              )}
            </div>

            <div className="flex items-center space-x-1">
              <button
                type="button"
                onClick={() => void fetchJobs()}
                title="Atualizar lista de jobs"
                aria-label="Atualizar lista"
                disabled={loading}
                className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer disabled:opacity-50"
              >
                <IconRefresh
                  className={`size-3.5 ${loading ? "animate-spin text-brand-400" : ""}`}
                />
              </button>
              <button
                type="button"
                onClick={onClose}
                aria-label="Fechar Centro de Atividades"
                className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer"
              >
                <IconX className="size-4" />
              </button>
            </div>
          </div>

          {/* Abas de Filtro de Jobs */}
          <div className="border-b border-white/10 px-3 py-2 bg-black/20">
            <SubmodulePills<"all" | "running" | "done" | "failed">
              size="sm"
              value={tab}
              onChange={setTab}
              items={[
                { id: "all", label: "Todos", count: sortedJobs.length },
                { id: "running", label: "Em execução", count: activeCount },
                { id: "done", label: "Concluídos", count: doneCount },
                { id: "failed", label: "Falhas", count: failedCount },
              ]}
            />
          </div>

          {/* Barra de Pesquisa */}
          <div className="border-b border-white/10 p-3">
            <SearchInput
              size="md"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onClear={() => setQuery("")}
              placeholder="Pesquisar por modelo, job ID ou tipo…"
              aria-label="Pesquisar jobs"
            />
          </div>

          {/* Lista de Jobs / Timeline */}
          <div className="flex-1 overflow-y-auto p-4 space-y-2.5">
            {filtered.length === 0 ? (
              <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-8 text-center mt-6">
                <span className="flex size-10 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
                  <IconTarget className="size-5 text-zinc-600" />
                </span>
                <p className="text-xs font-medium text-zinc-300">
                  {query
                    ? `Nenhum job encontrado para "${query}"`
                    : "Nenhum job no momento"}
                </p>
                <p className="text-[11px] text-zinc-500 max-w-xs">
                  Inicie um treinamento YOLO ou execute um AutoTracker para acompanhar o progresso em tempo real aqui.
                </p>
                <button
                  type="button"
                  onClick={() => {
                    onClose();
                    router.push("/jobs");
                  }}
                  className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-zinc-200 transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985]"
                >
                  <IconPlay className="size-3 text-brand-400" />
                  <span>Novo Treino YOLO</span>
                </button>
              </div>
            ) : (
              filtered.map((job) => {
                const config = STATUS_CONFIG[job.status] || STATUS_CONFIG.queued;
                const isExpanded = expandedId === job.id;
                const isActive =
                  job.status === "running" ||
                  job.status === "queued" ||
                  job.status === "cancelling";
                const isRunning = job.status === "running";
                const pct = Math.round((job.progress ?? 0) * 100);
                const duration = formatDuration(job.createdAt, job.finishedAt);
                const jobExtraMetrics = metrics[job.id];
                const jobExtraArtifacts = artifacts[job.id];
                const latestMetric =
                  jobExtraMetrics && jobExtraMetrics.length > 0
                    ? jobExtraMetrics[jobExtraMetrics.length - 1]
                    : null;

                const kindTitle =
                  job.kind === "yolo_train"
                    ? "Treino YOLO"
                    : job.kind === "autotracker"
                      ? "AutoTracker"
                      : job.kind;

                return (
                  <div
                    key={job.id}
                    className="group relative overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-900/60 backdrop-blur-sm transition-all hover:border-zinc-700 hover:bg-zinc-900/90"
                  >
                    {/* Linha vertical de status */}
                    <div
                      className={`absolute top-0 bottom-0 left-0 w-1 ${config.borderClass}`}
                      aria-hidden="true"
                    />

                    <div
                      className="p-3 pl-4 cursor-pointer"
                      onClick={() => toggleExpand(job.id)}
                    >
                      {/* Top row */}
                      <div className="flex items-start justify-between gap-2.5">
                        <div className="flex items-start space-x-2.5 min-w-0 flex-1">
                          {/* Ícone de status */}
                          <div
                            className={`mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full ${config.iconBg} ${config.iconColor}`}
                          >
                            {isRunning ? (
                              <IconRefresh className="size-3.5 animate-spin" />
                            ) : job.status === "done" ? (
                              <IconCheck className="size-3.5" />
                            ) : job.status === "failed" ? (
                              <IconX className="size-3.5" />
                            ) : (
                              <IconTarget className="size-3.5" />
                            )}
                          </div>

                          <div className="min-w-0 flex-1">
                            <div className="flex items-baseline gap-1.5 flex-wrap">
                              <span className="text-xs font-semibold text-zinc-100 truncate">
                                {job.model}
                              </span>
                              <span className="font-mono text-[10px] text-brand-400 shrink-0">
                                · {kindTitle}
                              </span>
                              <span className="font-mono text-[10px] text-zinc-500 shrink-0">
                                · {formatRelativeTime(job.createdAt)}
                              </span>
                            </div>

                            <p className="mt-0.5 font-mono text-[10px] text-zinc-400 truncate">
                              {job.engine} · Duração: {duration}
                              {job.queuePosition !== null &&
                                job.queuePosition !== undefined &&
                                ` · Fila: #${job.queuePosition}`}
                            </p>
                          </div>
                        </div>

                        {/* Status Badge + Chevron */}
                        <div className="flex items-center space-x-1.5 shrink-0">
                          <Badge
                            variant={jobStatusToBadgeVariant(job.status)}
                          >
                            {config.label}
                          </Badge>
                          <span
                            className={`text-zinc-500 transition-transform duration-200 ${
                              isExpanded ? "rotate-180" : ""
                            }`}
                          >
                            <IconChevronDown className="size-3.5" />
                          </span>
                        </div>
                      </div>

                      {/* Barra de Progresso em jobs ativos */}
                      {isActive && (
                        <div className="mt-2.5">
                          <div className="flex items-center justify-between text-[10px] font-mono text-zinc-400 mb-1">
                            <span>Progresso</span>
                            <span>{pct}%</span>
                          </div>
                          <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                            <div
                              className="h-full rounded-full bg-gradient-to-r from-brand-500 to-[#34d399] transition-all duration-500"
                              style={{ width: `${Math.max(4, pct)}%` }}
                            />
                          </div>
                        </div>
                      )}

                      {/* Painel expansível: Detalhes, Métricas, Ações */}
                      {isExpanded && (
                        <div
                          className="mt-3 border-t border-zinc-800/80 pt-3 text-[11px] font-mono space-y-3 bg-black/30 backdrop-blur-sm -mx-3 -mb-3 p-3"
                          onClick={(e) => e.stopPropagation()}
                        >
                          {/* Info chips */}
                          <div className="grid grid-cols-2 gap-2 text-[10px]">
                            <div className="rounded bg-black/40 backdrop-blur-sm p-2 border border-zinc-800/60">
                              <span className="text-zinc-500 block uppercase tracking-caps text-[9px]">
                                Job ID
                              </span>
                              <span className="text-zinc-300 font-mono truncate block" title={job.id}>
                                {job.id}
                              </span>
                            </div>

                            {job.datasetId ? (
                              <div className="rounded bg-black/40 backdrop-blur-sm p-2 border border-zinc-800/60 flex items-center justify-between">
                                <div className="min-w-0 flex-1 mr-1">
                                  <span className="text-zinc-500 block uppercase tracking-caps text-[9px]">
                                    Dataset
                                  </span>
                                  <span className="text-zinc-300 font-mono truncate block" title={job.datasetId}>
                                    {job.datasetId.slice(0, 8)}…
                                  </span>
                                </div>
                                <button
                                  type="button"
                                  onClick={() => {
                                    onClose();
                                    router.push(`/datasets/${job.datasetId}`);
                                  }}
                                  className="text-brand-400 hover:text-brand-300 text-[10px] underline underline-offset-2 shrink-0"
                                >
                                  Abrir
                                </button>
                              </div>
                            ) : (
                              <div className="rounded bg-black/40 backdrop-blur-sm p-2 border border-zinc-800/60">
                                <span className="text-zinc-500 block uppercase tracking-caps text-[9px]">
                                  Dataset
                                </span>
                                <span className="text-zinc-500">—</span>
                              </div>
                            )}
                          </div>

                          {/* Métricas ao vivo/finais se disponíveis */}
                          {latestMetric && (
                            <div>
                              <div className="text-[9px] text-zinc-400 uppercase tracking-caps mb-1.5">
                                Métricas (Epoch {latestMetric.epoch})
                              </div>
                              <div className="grid grid-cols-4 gap-1.5 text-center">
                                <div className="rounded bg-black/50 backdrop-blur-sm p-1.5 border border-zinc-800/80">
                                  <span className="text-[9px] text-zinc-500 block">mAP50</span>
                                  <span className="text-xs font-semibold text-[#34d399]">
                                    {(latestMetric.map50 * 100).toFixed(1)}%
                                  </span>
                                </div>
                                <div className="rounded bg-black/50 backdrop-blur-sm p-1.5 border border-zinc-800/80">
                                  <span className="text-[9px] text-zinc-500 block">mAP50-95</span>
                                  <span className="text-xs font-semibold text-[#34d399]">
                                    {(latestMetric.map5095 * 100).toFixed(1)}%
                                  </span>
                                </div>
                                <div className="rounded bg-black/50 backdrop-blur-sm p-1.5 border border-zinc-800/80">
                                  <span className="text-[9px] text-zinc-500 block">Box Loss</span>
                                  <span className="text-xs font-semibold text-zinc-200">
                                    {latestMetric.boxLoss?.toFixed(3) ?? "—"}
                                  </span>
                                </div>
                                <div className="rounded bg-black/50 backdrop-blur-sm p-1.5 border border-zinc-800/80">
                                  <span className="text-[9px] text-zinc-500 block">Cls Loss</span>
                                  <span className="text-xs font-semibold text-zinc-200">
                                    {latestMetric.clsLoss?.toFixed(3) ?? "—"}
                                  </span>
                                </div>
                              </div>
                            </div>
                          )}

                          {/* Artefatos disponíveis para download */}
                          {jobExtraArtifacts && jobExtraArtifacts.length > 0 && (
                            <div>
                              <div className="text-[9px] text-zinc-400 uppercase tracking-caps mb-1.5">
                                Artefatos Gerados ({jobExtraArtifacts.length})
                              </div>
                              <div className="space-y-1">
                                {jobExtraArtifacts.map((art) => (
                                  <div
                                    key={art.id}
                                    className="flex items-center justify-between rounded border border-zinc-800 bg-black/40 backdrop-blur-sm px-2.5 py-1 text-[10px]"
                                  >
                                    <span className="truncate text-zinc-300 mr-2" title={art.path}>
                                      {art.path.split("/").pop()} ({formatBytes(art.bytes)})
                                    </span>
                                    <button
                                      type="button"
                                      onClick={() => handleDownload(job.id, art)}
                                      className="inline-flex items-center gap-1 text-brand-400 hover:text-brand-300 font-medium shrink-0"
                                    >
                                      <IconDownload className="size-3" />
                                      <span>Baixar</span>
                                    </button>
                                  </div>
                                ))}
                              </div>
                            </div>
                          )}

                          {/* Mensagem de Erro se falhou */}
                          {job.queueReason && job.status === "failed" && (
                            <div className="rounded bg-rose-950/40 border border-rose-800/50 p-2 text-rose-300 text-[10px]">
                              {job.queueReason}
                            </div>
                          )}

                          {/* Ações contextuais */}
                          <div className="pt-2 border-t border-zinc-800/80 flex items-center justify-between gap-2 flex-wrap">
                            {/* AutoTracker: aplicar boxes */}
                            {job.kind === "autotracker" && job.status === "done" && (
                              <div className="flex items-center gap-2 flex-wrap">
                                <label className="flex items-center gap-1.5 text-[10px] text-zinc-400 cursor-pointer">
                                  <input
                                    type="checkbox"
                                    checked={applyOverwrite}
                                    onChange={(e) => setApplyOverwrite(e.target.checked)}
                                    className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3"
                                  />
                                  <span>Sobrescrever</span>
                                </label>
                                <button
                                  type="button"
                                  disabled={applyBusy}
                                  onClick={() => handleApplyBoxes(job)}
                                  className="inline-flex items-center gap-1 rounded-lg border border-[#34d399]/40 bg-[#34d399]/15 px-2.5 py-1 text-[10px] font-medium text-[#a7f3d0] transition hover:bg-[#34d399]/25 active:scale-[0.985] disabled:opacity-50"
                                >
                                  <IconCheck className="size-3" />
                                  <span>{applyBusy ? "Aplicando…" : "Aplicar ao dataset"}</span>
                                </button>
                              </div>
                            )}

                            {/* Cancelar Job ativo */}
                            {isActive && (
                              <button
                                type="button"
                                onClick={() => setAbortTarget(job)}
                                className="inline-flex items-center gap-1 rounded-lg border border-rose-500/40 bg-rose-500/15 px-2.5 py-1 text-[10px] font-medium text-rose-300 transition hover:bg-rose-500/25 active:scale-[0.985]"
                              >
                                <IconTrash className="size-3" />
                                <span>Cancelar Job</span>
                              </button>
                            )}

                            {/* Ver detalhes no studio */}
                            <button
                              type="button"
                              onClick={() => {
                                onClose();
                                router.push(`/jobs?selected=${job.id}`);
                              }}
                              className="ml-auto text-zinc-400 hover:text-zinc-200 text-[10px] underline underline-offset-2"
                            >
                              Ver na Forja →
                            </button>
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                );
              })
            )}
          </div>

          {/* Footer fixo do drawer com atalho para Treino YOLO */}
          <div className="border-t border-white/10 p-3 bg-black/40 backdrop-blur-sm flex items-center justify-between">
            <button
              type="button"
              onClick={() => {
                onClose();
                router.push("/jobs");
              }}
              className="w-full inline-flex items-center justify-center gap-2 rounded-xl border border-white/10 bg-white/[0.04] backdrop-blur-sm py-2 text-xs font-medium text-zinc-200 transition hover:border-white/20 hover:bg-white/[0.08] active:scale-[0.985]"
            >
              <IconTarget className="size-3.5 text-brand-400" />
              <span>Abrir Forja de Treino YOLO</span>
            </button>
          </div>
        </aside>
      </div>

      {/* Confirmação de cancelamento de Job */}
      <ConfirmDialog
        open={Boolean(abortTarget)}
        title="Cancelar execução do Job"
        body={
          <p className="text-xs text-zinc-300">
            Tem certeza de que deseja interromper a execução do job{" "}
            <strong className="text-white font-mono">{abortTarget?.model}</strong> (
            {abortTarget?.id.slice(0, 8)}…)? O processo de inferência/treino será encerrado imediatamente.
          </p>
        }
        confirmLabel="Sim, cancelar job"
        danger
        busy={abortBusy}
        onConfirm={handleAbort}
        onClose={() => setAbortTarget(null)}
      />
    </>
  );
}

export default ActionCenter;
