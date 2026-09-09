"use client";

import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import {
  abortJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  listJobs,
} from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import { Button } from "@/components/ui/Button";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import {
  ConvergenceChart,
  MetricSparkline,
} from "@/components/studio/ConvergenceChart";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import {
  IconActivity,
  IconCheck,
  IconDatabase,
  IconDownload,
  IconPlay,
  IconRefresh,
  IconTarget,
  IconTrash,
  IconZap,
} from "@/components/icons";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
} from "@/types/studio";
import { autotrackerErrorMessage } from "@/types/studio";
import { formatBytes, formatDuration, formatRelativeTime } from "@/lib/format";
import { openActionCenter } from "@/lib/events";
import { JobListItem } from "@/components/studio/JobCard";

const POLL_INTERVAL = 3000;

const SPARK_COLORS: Record<string, string> = {
  map50: "#34d399",
  map5095: "#2dd4bf",
  boxLoss: "#38bdf8",
  clsLoss: "#818cf8",
  dflLoss: "#fbbf24",
  epoch: "#a1a1aa",
};

const STATUS_LABEL: Record<JobStatus, string> = {
  queued: "Na fila",
  running: "Executando",
  cancelling: "Cancelando",
  done: "Concluído",
  failed: "Falhou",
  cancelled: "Cancelado",
};

/** Status que indicam job em andamento (ativação de polling). */
const ACTIVE_STATUSES: JobStatus[] = ["queued", "running", "cancelling"];

function isActive(status: JobStatus): boolean {
  return ACTIVE_STATUSES.includes(status);
}

function JobsPageContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
  const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
  const [abortTarget, setAbortTarget] = useState<Job | null>(null);
  const [abortBusy, setAbortBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [applyOverwrite, setApplyOverwrite] = useState(false);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Lê query param ?job=jobId para auto-seleção (usado pela navegação de /treino)
  useEffect(() => {
    const qJob = searchParams.get("job");
    if (qJob) {
      setSelectedJobId(qJob);
    }
  }, [searchParams]);

  // Resetar applyOverwrite ao trocar de job
  useEffect(() => {
    setApplyOverwrite(false);
  }, [selectedJobId]);

  const fetchJobs = useCallback(async () => {
    try {
      const data = await listJobs();
      setJobs(data.items);
      setError(null);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        return;
      }
      setError("Falha ao carregar lista de jobs.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  // Polling a cada 3s se houver jobs ativos
  useEffect(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }

    if (jobs.some((j) => isActive(j.status))) {
      pollRef.current = setInterval(() => {
        if (typeof document !== "undefined" && document.visibilityState === "hidden") {
          return;
        }
        void fetchJobs();
      }, POLL_INTERVAL);
    }

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible" && jobs.some((j) => isActive(j.status))) {
        void fetchJobs();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [jobs, fetchJobs]);

  // Jobs agrupados: ativos primeiro, depois terminais — ambos em ordem cronológica (mais recente primeiro)
  const { activeJobs, terminalJobs } = useMemo(() => {
    const sorted = [...jobs].sort(
      (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    );
    return {
      activeJobs: sorted.filter((j) => isActive(j.status)),
      terminalJobs: sorted.filter((j) => !isActive(j.status)),
    };
  }, [jobs]);

  const selectedJob = useMemo(() => {
    if (selectedJobId) {
      return jobs.find((j) => j.id === selectedJobId) ?? null;
    }
    // Auto-focus no job ativo mais recente, ou no mais recente terminal
    return activeJobs[0] ?? terminalJobs[0] ?? null;
  }, [jobs, selectedJobId, activeJobs, terminalJobs]);

  // Carregar métricas e artefatos quando o selectedJob mudar
  useEffect(() => {
    const targetId = selectedJob?.id;
    if (!targetId) return;
    const ctrl = new AbortController();

    async function loadDetail() {
      try {
        const [m, a] = await Promise.all([
          getJobMetrics(targetId!),
          getJobArtifacts(targetId!),
        ]);
        if (!ctrl.signal.aborted) {
          setMetrics((prev) => ({ ...prev, [targetId!]: m.items }));
          setArtifacts((prev) => ({ ...prev, [targetId!]: a.items }));
        }
      } catch {
        // Detalhe é best-effort
      }
    }

    loadDetail();
    return () => ctrl.abort();
  }, [selectedJob?.id]);

  // Re-fetch métricas e artefatos quando o job selecionado está ativo e jobs atualiza
  useEffect(() => {
    const targetId = selectedJob?.id;
    if (!targetId) return;
    const job = jobs.find((j) => j.id === targetId);
    if (!job || !isActive(job.status)) return;

    const ctrl = new AbortController();

    async function refreshDetail() {
      try {
        const [m, a] = await Promise.all([
          getJobMetrics(targetId!),
          getJobArtifacts(targetId!),
        ]);
        if (!ctrl.signal.aborted) {
          setMetrics((prev) => ({ ...prev, [targetId!]: m.items }));
          setArtifacts((prev) => ({ ...prev, [targetId!]: a.items }));
        }
      } catch {
        // Detalhe é best-effort
      }
    }

    refreshDetail();
    return () => ctrl.abort();
  }, [selectedJob?.id, jobs]);

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

  async function handleApplyBoxes(job: Job) {
    setApplyBusy(true);
    try {
      const result = await applyAutotrackerBoxes(job.id, {
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} boxes aplicadas, ${result.skipped} ignoradas em ${result.images} imagem(ns).`,
        "success",
        job.datasetId
          ? {
              label: "Abrir dataset",
              onClick: () => router.push(`/datasets/${job.datasetId}`),
            }
          : undefined,
      );
      setApplyOverwrite(false);
      await fetchJobs();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
        if (err.code === "job_not_done") {
          showToast("O job ainda não terminou — aguarde a conclusão.", "info");
          return;
        }
        showToast(autotrackerErrorMessage(err.code), "error");
        return;
      }
      showToast("Falha ao aplicar boxes.", "error");
    } finally {
      setApplyBusy(false);
    }
  }

  async function handleDownloadArtifact(jobId: string, art: JobArtifact) {
    try {
      await downloadArtifact(jobId, art.id, art.path);
    } catch {
      showToast("Falha ao baixar artefato.", "error");
    }
  }

  const totalCount = activeJobs.length + terminalJobs.length;

  return (
    <div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
      {/* ═══════════════════════════════════════════════
          HEADER — EXECUÇÕES
          ═══════════════════════════════════════════════ */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
              <IconActivity className="size-4" />
            </span>
            <h1 className="font-display text-lg font-bold text-white tracking-tight">
              Execuções
            </h1>
            {totalCount > 0 && (
              <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[11px] text-zinc-400 backdrop-blur-sm">
                {totalCount} {totalCount === 1 ? "execução" : "execuções"}
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-zinc-400 max-w-2xl">
            Fila de trabalho em execução e histórico de todos os tipos (YOLO, AutoTracker).
          </p>
        </div>

        <div className="flex items-center space-x-2.5 shrink-0">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => void fetchJobs()}
            disabled={loading}
            title="Atualizar lista e status dos jobs"
          >
            <IconRefresh className={`size-3.5 ${loading ? "animate-spin text-brand-400" : ""}`} />
            <span>Atualizar</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={openActionCenter}
            title="Abrir Centro de Atividades lateral"
          >
            <IconZap className="size-3.5 text-brand-400" />
            <span className="hidden sm:inline">Centro de Atividades</span>
          </Button>
          <Link
            href="/treino"
            className="inline-flex items-center gap-2 h-8 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-3 text-xs font-medium text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] transition focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)]"
          >
            <IconPlay className="size-3.5" />
            <span>Novo Treino</span>
          </Link>
        </div>
      </div>

      {/* ═══════════════════════════════════════════════
          WORKSPACE DE 2 COLUNAS: LISTA + DETALHE
          ═══════════════════════════════════════════════ */}
      {!error && (
        <div className="flex flex-col md:flex-row items-start gap-6">
          {/* Coluna 1: Lista de Execuções (Fixa: w-full md:w-80 lg:w-96 shrink-0) */}
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
                  <p className="text-[11px] text-zinc-400">
                    Inicie um treino em{" "}
                    <Link href="/treino" className="text-brand-400 hover:text-brand-300 underline underline-offset-2">
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
                      <h3 className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400">
                        Em Execução
                      </h3>
                      <span className="flex items-center gap-1.5 font-mono text-[11px] text-brand-300">
                        <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
                        {activeJobs.length}
                      </span>
                    </div>
                    <div className="space-y-2">
                      {activeJobs.map((job) => (
                        <JobListItem
                          key={job.id}
                          job={job}
                          isFocused={selectedJob?.id === job.id}
                          onSelect={(id) => setSelectedJobId(id)}
                        />
                      ))}
                    </div>
                  </div>
                )}

                {/* ── Jobs Terminais ── */}
                {terminalJobs.length > 0 && (
                  <div className="space-y-2">
                    <div className="flex items-center justify-between px-1">
                      <h3 className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400">
                        Histórico
                      </h3>
                      <span className="font-mono text-[11px] text-zinc-500">
                        {terminalJobs.length}
                      </span>
                    </div>
                    <div className="space-y-2">
                      {terminalJobs.map((job) => (
                        <JobListItem
                          key={job.id}
                          job={job}
                          isFocused={selectedJob?.id === job.id}
                          onSelect={(id) => setSelectedJobId(id)}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </aside>

          {/* Coluna 2: Painel de Detalhe (flex-1 min-w-0) */}
          <section className="w-full flex-1 min-w-0 space-y-4">
            {selectedJob ? (
              <div className="space-y-4">
                {/* Cabeçalho do Job Selecionado */}
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <h2 className="font-display text-sm font-semibold text-zinc-200">
                      {isActive(selectedJob.status)
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
                  {selectedJobId && selectedJobId !== activeJobs[0]?.id && (
                    <button
                      type="button"
                      onClick={() => setSelectedJobId(null)}
                      className="text-xs text-zinc-400 hover:text-zinc-200 transition underline underline-offset-2"
                    >
                      {activeJobs.length > 0 ? "Voltar ao job ativo" : "Ver último job"}
                    </button>
                  )}
                </div>

                {/* Job Hero Card */}
                <div className="glass-card rounded-2xl p-5 space-y-4 border border-white/10">
                  <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                    <div>
                      <div className="flex items-center gap-2">
                        <span className="font-display text-base font-semibold text-zinc-100">
                          {selectedJob.model}
                        </span>
                        <span className="font-mono text-[11px] text-zinc-400">
                          · {selectedJob.kind} · {selectedJob.engine}
                        </span>
                      </div>
                      <div className="mt-1 flex items-center gap-3 font-mono text-[11px] text-zinc-400">
                        <span>
                          Duração: {formatDuration(selectedJob.createdAt, selectedJob.finishedAt)}
                        </span>
                        {selectedJob.epoch != null && (
                          <span>Epoch {selectedJob.epoch}</span>
                        )}
                        {isActive(selectedJob.status) && (
                          <span className="text-brand-300 font-semibold">
                            {Math.round((selectedJob.progress ?? 0) * 100)}%
                          </span>
                        )}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 font-mono text-[11px] text-zinc-400">
                      <span title={selectedJob.id}>ID: {selectedJob.id.slice(0, 8)}…</span>
                      {selectedJob.datasetId && (
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => router.push(`/datasets/${selectedJob.datasetId}`)}
                          leftIcon={<IconDatabase className="size-3" />}
                        >
                          Dataset
                        </Button>
                      )}
                    </div>
                  </div>

                  {/* Barra de progresso para jobs ativos */}
                  {isActive(selectedJob.status) && (
                    <div className="space-y-1.5 pt-1">
                      <div className="flex items-center justify-between font-mono text-[11px]">
                        <span className="text-zinc-400">Progresso</span>
                        <span className="font-semibold text-brand-300">
                          {Math.round((selectedJob.progress ?? 0) * 100)}%
                        </span>
                      </div>
                      <div className="h-2 w-full overflow-hidden rounded-full bg-black/40 border border-white/10">
                        <div
                          className="h-full rounded-full bg-gradient-to-r from-brand-600 via-brand-500 to-brand-400 transition-all duration-500 motion-reduce:transition-none"
                          style={{
                            width: `${Math.min(
                              100,
                              Math.max(3, Math.round((selectedJob.progress ?? 0) * 100)),
                            )}%`,
                          }}
                        />
                      </div>
                    </div>
                  )}

                  {/* Métricas da Execução & Curvas de Convergência */}
                  {(selectedJob.kind === "yolo_train" ||
                    (metrics[selectedJob.id] && metrics[selectedJob.id].length > 0)) && (
                    <div className="space-y-4 pt-3 border-t border-white/10">
                      <ConvergenceChart
                        metrics={metrics[selectedJob.id] || []}
                        totalEpochs={selectedJob.epoch || 100}
                        isJobActive={selectedJob.status === "running"}
                      />

                      {metrics[selectedJob.id] && metrics[selectedJob.id].length > 0 && (
                        <div className="space-y-2">
                          <div className="flex items-center justify-between">
                            <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                              Métricas (Epoch{" "}
                              {metrics[selectedJob.id]![metrics[selectedJob.id]!.length - 1].epoch}
                              )
                            </h3>
                            <span className="font-mono text-[11px] text-zinc-400">
                              {metrics[selectedJob.id]!.length} checkpoint(s)
                            </span>
                          </div>
                          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-2.5">
                            {(
                              [
                                ["map50", "mAP@50", true],
                                ["map5095", "mAP@50-95", true],
                                ["boxLoss", "Box Loss", false],
                                ["clsLoss", "Cls Loss", false],
                                ["dflLoss", "Dfl Loss", false],
                                ["epoch", "Epochs", false],
                              ] as const
                            ).map(([key, label, isPercent]) => {
                              const jobMetrics = metrics[selectedJob.id]!;
                              const last = jobMetrics[jobMetrics.length - 1];
                              const val = last[key as keyof JobMetricsType];
                              const isPrimary = key === "map50";
                              const series = jobMetrics.map(
                                (m) => m[key as keyof JobMetricsType] as number,
                              );
                              return (
                                <div
                                  key={key}
                                  className={`rounded-xl border p-3 flex flex-col justify-between backdrop-blur-sm ${
                                    isPrimary
                                      ? "border-brand-500/30 bg-brand-500/10"
                                      : "border-white/10 bg-white/[0.02]"
                                  }`}
                                >
                                  <div>
                                    <span
                                      className={`block font-mono text-[11px] font-medium tracking-caps uppercase ${
                                        isPrimary ? "text-brand-300" : "text-zinc-400"
                                      }`}
                                    >
                                      {label}
                                    </span>
                                    <span className="block font-mono text-base font-semibold text-zinc-100 mt-1">
                                      {typeof val === "number"
                                        ? isPercent
                                           ? `${(val * 100).toFixed(1)}%`
                                           : key === "epoch"
                                             ? val
                                             : val.toFixed(4)
                                        : "—"}
                                    </span>
                                  </div>
                                  <div className="mt-2 pt-2 border-t border-white/[0.04]">
                                    <MetricSparkline
                                      data={series}
                                      color={SPARK_COLORS[key] || "#34d399"}
                                    />
                                  </div>
                                </div>
                              );
                            })}
                          </div>
                        </div>
                      )}
                    </div>
                  )}

                  {/* Artefatos Gerados */}
                  {artifacts[selectedJob.id] && artifacts[selectedJob.id].length > 0 && (
                    <div className="space-y-3 pt-3 border-t border-white/10">
                      <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                        Artefatos ({artifacts[selectedJob.id].length})
                      </h3>
                      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2.5">
                        {artifacts[selectedJob.id].map((art) => (
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
                              <span className="block font-mono text-[11px] text-zinc-400">
                                {formatBytes(art.bytes)} · {art.kind}
                              </span>
                            </div>
                            <Button
                              type="button"
                              variant="secondary"
                              size="sm"
                              onClick={() => handleDownloadArtifact(selectedJob.id, art)}
                            >
                              <IconDownload className="size-3.5" />
                              <span>Baixar</span>
                            </Button>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Ações do Job */}
                  <div className="flex items-center justify-between gap-3 pt-2">
                    {selectedJob.kind === "autotracker" && selectedJob.status === "done" && (
                      <div className="flex items-center gap-3">
                        <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
                          <input
                            type="checkbox"
                            checked={applyOverwrite}
                            onChange={(e) => setApplyOverwrite(e.target.checked)}
                            className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40"
                          />
                          <span>Sobrescrever anotações existentes</span>
                        </label>
                        <Button
                          type="button"
                          variant="primary"
                          size="sm"
                          disabled={applyBusy}
                          loading={applyBusy}
                          onClick={() => handleApplyBoxes(selectedJob)}
                        >
                          <IconCheck className="size-3.5 text-brand-400" />
                          <span>{applyBusy ? "Aplicando…" : "Aplicar boxes ao dataset"}</span>
                        </Button>
                      </div>
                    )}

                    {isActive(selectedJob.status) && (
                      <Button
                        type="button"
                        variant="destructive"
                        size="sm"
                        onClick={() => setAbortTarget(selectedJob)}
                      >
                        <IconTrash className="size-3.5" />
                        <span>Cancelar Execução</span>
                      </Button>
                    )}
                  </div>
                </div>

                {/* Log Viewer */}
                <JobLogViewer
                  job={selectedJob}
                  metrics={metrics[selectedJob.id] || []}
                  artifacts={artifacts[selectedJob.id] || []}
                />
              </div>
            ) : (
              /* Empty State */
              <div className="glass-card flex flex-col items-center gap-3.5 rounded-2xl p-12 text-center border border-white/10">
                <span className="flex size-12 items-center justify-center rounded-xl border border-white/10 bg-white/5 text-zinc-400 backdrop-blur-sm">
                  <IconTarget className="size-6 text-brand-400/60" />
                </span>
                <div className="max-w-md space-y-1">
                  <h3 className="font-display text-sm font-semibold text-zinc-200">
                    Nenhuma execução selecionada
                  </h3>
                  <p className="text-xs text-zinc-400">
                    Selecione uma execução na lista ao lado para ver detalhes, métricas e artefatos.
                  </p>
                </div>
              </div>
            )}
          </section>
        </div>
      )}

      {/* Confirmação de Abort */}
      <ConfirmDialog
        open={Boolean(abortTarget)}
        title="Cancelar execução do Job"
        body={
          <p className="text-xs text-zinc-300">
            Tem certeza de que deseja interromper o job{" "}
            <strong className="text-white font-mono">{abortTarget?.model}</strong> (
            {abortTarget?.id.slice(0, 8)}…)? O processo será interrompido imediatamente.
          </p>
        }
        confirmLabel="Sim, cancelar job"
        danger
        busy={abortBusy}
        onConfirm={handleAbort}
        onClose={() => setAbortTarget(null)}
      />
    </div>
  );
}

export default function JobsPage() {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center p-8">
          <div className="size-8 animate-spin rounded-full border-2 border-brand-500 border-t-transparent" />
        </div>
      }
    >
      <JobsPageContent />
    </Suspense>
  );
}
