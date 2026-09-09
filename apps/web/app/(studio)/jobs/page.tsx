"use client";

import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
  abortJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  getTelemetry,
  listJobs,
} from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import ForjaYoloSetup from "@/components/studio/ForjaYoloSetup";
import { Button } from "@/components/ui/Button";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import {
  ConvergenceChart,
  MetricSparkline,
} from "@/components/studio/ConvergenceChart";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import {
  IconCheck,
  IconDatabase,
  IconDownload,
  IconLayers,
  IconPlay,
  IconRefresh,
  IconTarget,
  IconTrash,
  IconX,
  IconZap,
} from "@/components/icons";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
  Telemetry,
} from "@/types/studio";
import { autotrackerErrorMessage } from "@/types/studio";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import { openActionCenter } from "@/lib/events";

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
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Lê query param ?selected=jobId se fornecido na navegação
  useEffect(() => {
    const qSelected = searchParams.get("selected");
    if (qSelected) {
      setSelectedJobId(qSelected);
    }
  }, [searchParams]);

  // Resetar applyOverwrite ao trocar de job
  useEffect(() => {
    setApplyOverwrite(false);
  }, [selectedJobId]);

  const fetchTelemetry = useCallback(async () => {
    try {
      const data = await getTelemetry();
      setTelemetry(data);
    } catch {
      // Best-effort
    }
  }, []);

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
    fetchTelemetry();
  }, [fetchJobs, fetchTelemetry]);

  // Polling a cada 3s se houver jobs ativos
  useEffect(() => {
    function hasActiveJobs(list: Job[]) {
      return list.some(
        (j) =>
          j.status === "queued" ||
          j.status === "running" ||
          j.status === "cancelling",
      );
    }

    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }

    if (hasActiveJobs(jobs)) {
      pollRef.current = setInterval(() => {
        if (typeof document !== "undefined" && document.visibilityState === "hidden") {
          return;
        }
        void fetchJobs();
      }, POLL_INTERVAL);
    }

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible" && hasActiveJobs(jobs)) {
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

  const sorted = useMemo(
    () =>
      [...jobs].sort(
        (a, b) =>
          new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
      ),
    [jobs],
  );

  const activeJob = useMemo(
    () =>
      jobs.find(
        (j) =>
          j.status === "running" ||
          j.status === "queued" ||
          j.status === "cancelling",
      ) ?? null,
    [jobs],
  );

  const selectedJob = useMemo(() => {
    if (selectedJobId) {
      return jobs.find((j) => j.id === selectedJobId) ?? null;
    }
    // Auto-focus no job ativo, ou no mais recente concluído
    return activeJob ?? sorted[0] ?? null;
  }, [jobs, selectedJobId, activeJob, sorted]);

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
    if (!job) return;

    const isActive =
      job.status === "queued" ||
      job.status === "running" ||
      job.status === "cancelling";
    if (!isActive) return;

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

  function handleJobCreated(jobId: string) {
    setSelectedJobId(jobId);
    void fetchJobs();
    openActionCenter();
  }

  const activeJobsCount = jobs.filter(
    (j) =>
      j.status === "running" ||
      j.status === "queued" ||
      j.status === "cancelling",
  ).length;

  return (
    <div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
      {/* ═══════════════════════════════════════════════
          HEADER DA FORJA
          ═══════════════════════════════════════════════ */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
              <IconTarget className="size-4" />
            </span>
            <h1 className="font-display text-lg font-bold text-white tracking-tight">
              Forja de Treino YOLO
            </h1>
            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[11px] uppercase tracking-caps text-zinc-400 backdrop-blur-sm">
              Ultralytics Engine
            </span>
          </div>
          <p className="mt-1 text-xs text-zinc-400 max-w-2xl">
            Configure hiperparâmetros, selecione datasets e execute o treinamento local com telemetria em tempo real.
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
        </div>
      </div>

      {/* ═══════════════════════════════════════════════
          WORKSPACE DE 2 COLUNAS CANÔNICO DO STUDIO
          ═══════════════════════════════════════════════ */}
      {!error && (
        <div className="flex flex-col md:flex-row items-start gap-6">
          {/* Coluna 1: Setup & Controle (Fixa: w-full md:w-80 lg:w-96 shrink-0) */}
          <aside className="w-full md:w-80 lg:w-96 shrink-0 md:sticky md:top-4 md:max-h-[calc(100vh-2rem)] md:overflow-y-auto overflow-x-hidden [scrollbar-width:thin]">
            <div className="glass-card rounded-2xl p-5 border border-white/10">
              <ForjaYoloSetup
                onJobCreated={handleJobCreated}
                initialTelemetry={telemetry}
              />
            </div>
          </aside>

          {/* Coluna 2: Canvas de Monitoramento Fluido & Histórico (flex-1 min-w-0) */}
          <section className="w-full flex-1 min-w-0 space-y-6">
            {/* Monitor Stage: Job Ativo ou Selecionado */}
            {selectedJob ? (
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <h2 className="font-display text-sm font-semibold text-zinc-200">
                      {selectedJob.status === "running" ||
                      selectedJob.status === "queued" ||
                      selectedJob.status === "cancelling"
                        ? "Execução Ativa no Nó Local"
                        : "Painel de Execução & Métricas"}
                    </h2>
                    <Badge
                      variant={jobStatusToBadgeVariant(selectedJob.status)}
                      pulse={selectedJob.status === "running"}
                    >
                      {STATUS_LABEL[selectedJob.status]}
                    </Badge>
                  </div>
                  {selectedJobId && selectedJobId !== activeJob?.id && (
                    <button
                      type="button"
                      onClick={() => setSelectedJobId(null)}
                      className="text-xs text-zinc-400 hover:text-zinc-200 transition underline underline-offset-2"
                    >
                      {activeJob ? "Voltar ao job ativo" : "Ver último job"}
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
                        {(selectedJob.status === "queued" ||
                          selectedJob.status === "running") && (
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

                  {/* Barra de progresso full-width para jobs ativos */}
                  {(selectedJob.status === "queued" ||
                    selectedJob.status === "running" ||
                    selectedJob.status === "cancelling") && (
                    <div className="space-y-1.5 pt-1">
                      <div className="flex items-center justify-between font-mono text-[11px]">
                        <span className="text-zinc-400">Progresso do Treinamento</span>
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
                      {/* Curvas de Convergência Vetoriais (SVG Multi-Curve) */}
                      <ConvergenceChart
                        metrics={metrics[selectedJob.id] || []}
                        totalEpochs={selectedJob.epoch || 100}
                        isJobActive={selectedJob.status === "running"}
                      />

                      {/* Cards com Sparklines Integradas */}
                      {metrics[selectedJob.id] && metrics[selectedJob.id].length > 0 && (
                        <div className="space-y-2">
                          <div className="flex items-center justify-between">
                            <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                              Métricas da Execução (Epoch{" "}
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

                  {/* Artefatos Gerados (Achatado - sem nested-cards) */}
                  {artifacts[selectedJob.id] && artifacts[selectedJob.id].length > 0 && (
                    <div className="space-y-3 pt-3 border-t border-white/10">
                      <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                        Artefatos Gerados ({artifacts[selectedJob.id].length})
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

                    {(selectedJob.status === "queued" || selectedJob.status === "running") && (
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

                {/* Visualizador Colapsável de Streaming de Logs do Orquestrador */}
                <JobLogViewer
                  job={selectedJob}
                  metrics={metrics[selectedJob.id] || []}
                  artifacts={artifacts[selectedJob.id] || []}
                />
              </div>
            ) : (
              /* Empty State quando não há nenhum job */
              <div className="glass-card flex flex-col items-center gap-3.5 rounded-2xl p-12 text-center border border-white/10">
                <span className="flex size-12 items-center justify-center rounded-xl border border-white/10 bg-white/5 text-zinc-400 backdrop-blur-sm">
                  <IconTarget className="size-6 text-brand-400/60" />
                </span>
                <div className="max-w-md space-y-1">
                  <h3 className="font-display text-sm font-semibold text-zinc-200">
                    Nenhum treinamento registrado
                  </h3>
                  <p className="text-xs text-zinc-400">
                    Configure os hiperparâmetros no painel lateral à esquerda e clique em{" "}
                    <strong className="text-zinc-200">Iniciar Treino</strong> para disparar a
                    primeira execução local no nó.
                  </p>
                </div>
              </div>
            )}

            {/* ═══════════════════════════════════════════════
                HISTÓRICO COMPLETO DE JOBS
                ═══════════════════════════════════════════════ */}
            <div className="space-y-3.5 pt-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <h3 className="font-display text-sm font-semibold text-zinc-200">
                    Histórico de Execuções
                  </h3>
                  <span className="rounded-full border border-white/10 bg-white/5 px-2.5 py-0.5 font-mono text-[11px] text-zinc-400 backdrop-blur-sm">
                    {sorted.length} {sorted.length === 1 ? "execução" : "execuções"}
                  </span>
                </div>
                {activeJobsCount > 0 && (
                  <span className="flex items-center gap-1.5 font-mono text-[11px] text-brand-300">
                    <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
                    {activeJobsCount} ativo(s)
                  </span>
                )}
              </div>

              {loading ? (
                <div className="glass-card rounded-2xl p-8 text-center text-xs text-zinc-400 font-mono border border-white/10">
                  Carregando histórico de jobs…
                </div>
              ) : sorted.length === 0 ? (
                <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-8 text-center border border-white/10">
                  <p className="text-xs font-medium text-zinc-400">
                    Nenhum histórico disponível até o momento.
                  </p>
                </div>
              ) : (
                <div className="grid grid-cols-1 gap-2.5">
                  {sorted.map((job) => {
                    const isActive =
                      job.status === "queued" ||
                      job.status === "running" ||
                      job.status === "cancelling";
                    const isFocused = selectedJob?.id === job.id;
                    const pct = Math.round((job.progress ?? 0) * 100);

                    return (
                      <div
                        key={job.id}
                        onClick={() => setSelectedJobId(job.id)}
                        className={`glass-card group relative flex flex-col sm:flex-row sm:items-center justify-between gap-3.5 rounded-xl p-4 transition-all duration-200 cursor-pointer border ${
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
                            {STATUS_LABEL[job.status]}
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
                            </div>
                          </div>
                        </div>

                        <div className="flex items-center space-x-2 shrink-0 pl-1 sm:pl-0">
                          <Button
                            type="button"
                            variant={isFocused ? "primary" : "secondary"}
                            size="sm"
                            onClick={(e) => {
                              e.stopPropagation();
                              setSelectedJobId(job.id);
                            }}
                          >
                            <span>{isFocused ? "Em exibição" : "Ver detalhes"}</span>
                            <span aria-hidden="true">→</span>
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
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

