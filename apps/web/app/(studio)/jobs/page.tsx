"use client";

import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
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
import ForjaYoloSetup from "@/components/studio/ForjaYoloSetup";
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
} from "@/types/studio";
import { autotrackerErrorMessage } from "@/types/studio";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import { openActionCenter } from "@/lib/events";

const POLL_INTERVAL = 3000;

const STATUS_STYLE: Record<JobStatus, string> = {
  queued: "border-amber-500/30 bg-amber-500/10 text-amber-300",
  running: "border-brand-500/35 bg-brand-500/15 text-brand-300",
  cancelling: "border-amber-500/30 bg-amber-500/10 text-amber-300",
  done: "border-[#34d399]/30 bg-[#34d399]/10 text-[#a7f3d0]",
  failed: "border-rose-500/30 bg-rose-500/10 text-rose-300",
  cancelled: "border-zinc-700 bg-zinc-800 text-zinc-400",
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

type ViewMode = "setup" | "history";

function JobsPageContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const [viewMode, setViewMode] = useState<ViewMode>("setup");
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

  // Lê query param ?selected=jobId se fornecido na navegação
  useEffect(() => {
    const qSelected = searchParams.get("selected");
    if (qSelected) {
      setSelectedJobId(qSelected);
      setViewMode("history");
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
        void fetchJobs();
      }, POLL_INTERVAL);
    }

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [jobs, fetchJobs]);

  // Carregar métricas e artefatos quando um job é selecionado
  useEffect(() => {
    if (!selectedJobId) return;
    const ctrl = new AbortController();

    async function loadDetail() {
      try {
        const [m, a] = await Promise.all([
          getJobMetrics(selectedJobId!),
          getJobArtifacts(selectedJobId!),
        ]);
        if (!ctrl.signal.aborted) {
          setMetrics((prev) => ({ ...prev, [selectedJobId!]: m.items }));
          setArtifacts((prev) => ({ ...prev, [selectedJobId!]: a.items }));
        }
      } catch {
        // Detalhe é best-effort
      }
    }

    loadDetail();
    return () => ctrl.abort();
  }, [selectedJobId]);

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

  const sorted = useMemo(
    () =>
      [...jobs].sort(
        (a, b) =>
          new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
      ),
    [jobs],
  );

  const selectedJob = selectedJobId
    ? jobs.find((j) => j.id === selectedJobId) ?? null
    : null;

  const activeJobsCount = jobs.filter(
    (j) =>
      j.status === "running" ||
      j.status === "queued" ||
      j.status === "cancelling",
  ).length;

  return (
    <div className="mx-auto max-w-5xl px-4 py-6 md:px-6 space-y-6">
      {/* ═══════════════════════════════════════════════
          HEADER DA FORJA & NAVEGAÇÃO DE ABAS
          ═══════════════════════════════════════════════ */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-zinc-800/80 pb-5">
        <div>
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
              <IconTarget className="size-4" />
            </span>
            <h1 className="font-display text-lg font-bold text-white tracking-tight">
              Forja de Treino YOLO
            </h1>
            <span className="rounded-full border border-zinc-800 bg-zinc-900/90 px-2 py-0.5 font-mono text-[10px] text-zinc-400">
              Ultralytics Engine
            </span>
          </div>
          <p className="mt-1 text-xs text-zinc-400">
            Configure hiperparâmetros, selecione datasets e execute o treinamento local com telemetria em tempo real.
          </p>
        </div>

        <div className="flex items-center space-x-2 shrink-0">
          {/* Alternador de Abas */}
          <div className="flex items-center rounded-xl border border-white/10 bg-black/40 p-1">
            <button
              type="button"
              onClick={() => {
                setSelectedJobId(null);
                setViewMode("setup");
              }}
              className={`rounded-lg px-3 py-1.5 text-xs font-medium transition ${
                viewMode === "setup" && !selectedJob
                  ? "border border-white/15 bg-white/10 text-white shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              Setup de Treino
            </button>
            <button
              type="button"
              onClick={() => setViewMode("history")}
              className={`flex items-center space-x-1.5 rounded-lg px-3 py-1.5 text-xs font-medium transition ${
                viewMode === "history" || selectedJob
                  ? "border border-white/15 bg-white/10 text-white shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200"
              }`}
            >
              <span>Histórico & Detalhes</span>
              {activeJobsCount > 0 && (
                <span className="size-1.5 rounded-full bg-brand-400 animate-pulse" />
              )}
            </button>
          </div>

          {/* Botão de abrir o Centro de Atividades na lateral */}
          <button
            type="button"
            onClick={openActionCenter}
            title="Abrir Centro de Atividades lateral"
            className="inline-flex items-center gap-1.5 rounded-xl border border-brand-500/30 bg-brand-500/10 px-3 py-1.5 text-xs font-medium text-brand-300 transition hover:bg-brand-500/20 active:scale-[0.985] cursor-pointer"
          >
            <IconZap className="size-3.5 text-brand-400" />
            <span className="hidden sm:inline">Centro de Atividades</span>
          </button>
        </div>
      </div>

      {/* Error global */}
      {error && (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-8 text-center">
          <p className="text-sm text-zinc-300">{error}</p>
          <button
            type="button"
            onClick={() => {
              setLoading(true);
              void fetchJobs();
            }}
            className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3.5 py-1.5 text-xs font-medium text-zinc-200 hover:bg-white/10"
          >
            <IconRefresh className="size-3.5" />
            <span>Tentar novamente</span>
          </button>
        </div>
      )}

      {/* ═══════════════════════════════════════════════
          MODO 1: SETUP DE TREINO YOLO (FORJA)
          ═══════════════════════════════════════════════ */}
      {!error && viewMode === "setup" && !selectedJob && (
        <div className="min-w-0">
          <ForjaYoloSetup onJobCreated={handleJobCreated} />
        </div>
      )}

      {/* ═══════════════════════════════════════════════
          MODO 2: DETALHES DE UM JOB ESPECÍFICO SELECIONADO
          ═══════════════════════════════════════════════ */}
      {!error && selectedJob && (
        <div className="space-y-5">
          {/* Voltar ao histórico / setup */}
          <div className="flex items-center justify-between">
            <button
              type="button"
              onClick={() => {
                setSelectedJobId(null);
                setViewMode("setup");
              }}
              className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-zinc-300 transition hover:bg-white/[0.10] hover:text-white active:scale-[0.985]"
            >
              ← Voltar ao Setup de Treino
            </button>
            <button
              type="button"
              onClick={() => setSelectedJobId(null)}
              className="text-xs text-zinc-400 hover:text-zinc-200 underline underline-offset-2"
            >
              Ver todos os jobs
            </button>
          </div>

          {/* Job Header Card */}
          <div className="glass-card rounded-2xl p-5">
            <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
              <div className="flex items-center gap-3">
                <span
                  className={`shrink-0 rounded-full border px-2.5 py-1 font-mono text-[11px] font-medium ${STATUS_STYLE[selectedJob.status]}`}
                >
                  {STATUS_LABEL[selectedJob.status]}
                </span>
                <div>
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-zinc-100">
                      {selectedJob.model}
                    </span>
                    <span className="font-mono text-[10px] text-zinc-500">
                      {selectedJob.kind} · {selectedJob.engine}
                    </span>
                  </div>
                  <div className="mt-0.5 flex items-center gap-3 font-mono text-[10px] text-zinc-400">
                    <span>
                      Duração: {formatDuration(selectedJob.createdAt, selectedJob.finishedAt)}
                    </span>
                    {selectedJob.epoch != null && (
                      <span>Epoch {selectedJob.epoch}</span>
                    )}
                    {(selectedJob.status === "queued" ||
                      selectedJob.status === "running") && (
                      <span className="text-brand-300">
                        {Math.round((selectedJob.progress ?? 0) * 100)}%
                      </span>
                    )}
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2 font-mono text-[10px] text-zinc-400">
                <span title={selectedJob.id}>ID: {selectedJob.id.slice(0, 8)}…</span>
                {selectedJob.datasetId && (
                  <button
                    type="button"
                    onClick={() => router.push(`/datasets/${selectedJob.datasetId}`)}
                    className="inline-flex items-center gap-1 rounded border border-zinc-800 bg-black/40 px-2 py-0.5 text-brand-400 hover:text-brand-300"
                  >
                    <IconDatabase className="size-3" />
                    <span>Dataset</span>
                  </button>
                )}
              </div>
            </div>

            {/* Barra de progresso full-width */}
            {(selectedJob.status === "queued" ||
              selectedJob.status === "running") && (
              <div className="mt-4 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-gradient-to-r from-brand-500 to-[#34d399] transition-all duration-500"
                  style={{
                    width: `${Math.max(4, Math.round((selectedJob.progress ?? 0) * 100))}%`,
                  }}
                />
              </div>
            )}
          </div>

          {/* Métricas */}
          {metrics[selectedJob.id] && metrics[selectedJob.id].length > 0 && (
            <div className="glass-card rounded-2xl p-5">
              <h4 className="tracking-caps mb-3 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                Métricas da Execução (Epoch{" "}
                {metrics[selectedJob.id]![metrics[selectedJob.id]!.length - 1].epoch}
                )
              </h4>
              <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-6 gap-2.5">
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
                  return (
                    <div
                      key={key}
                      className="rounded-xl border border-zinc-800 bg-zinc-900/60 p-3"
                    >
                      <span className="block font-mono text-[10px] text-zinc-500">
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
                  );
                })}
              </div>
            </div>
          )}

          {/* Artefatos */}
          {artifacts[selectedJob.id] && artifacts[selectedJob.id].length > 0 && (
            <div className="glass-card rounded-2xl p-5">
              <h4 className="tracking-caps mb-3 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                Artefatos Gerados ({artifacts[selectedJob.id].length})
              </h4>
              <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 gap-2.5">
                {artifacts[selectedJob.id].map((art) => (
                  <div
                    key={art.id}
                    className="flex items-center justify-between rounded-xl border border-zinc-800 bg-black/40 p-3"
                  >
                    <div className="min-w-0 mr-2">
                      <span className="block text-xs font-semibold text-zinc-200 truncate">
                        {art.path.split("/").pop()}
                      </span>
                      <span className="block font-mono text-[10px] text-zinc-500">
                        {formatBytes(art.bytes)} · {art.kind}
                      </span>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleDownloadArtifact(selectedJob.id, art)}
                      className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.06] px-2.5 py-1.5 text-xs font-medium text-zinc-200 hover:bg-white/15 active:scale-[0.985]"
                    >
                      <IconDownload className="size-3.5" />
                      <span>Baixar</span>
                    </button>
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
                <button
                  type="button"
                  disabled={applyBusy}
                  onClick={() => handleApplyBoxes(selectedJob)}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-[#34d399]/40 bg-[#34d399]/15 px-3 py-1.5 text-xs font-medium text-[#a7f3d0] transition hover:bg-[#34d399]/25 active:scale-[0.985] disabled:opacity-50"
                >
                  <IconCheck className="size-3.5" />
                  <span>{applyBusy ? "Aplicando…" : "Aplicar boxes ao dataset"}</span>
                </button>
              </div>
            )}

            {(selectedJob.status === "queued" || selectedJob.status === "running") && (
              <button
                type="button"
                onClick={() => setAbortTarget(selectedJob)}
                className="inline-flex items-center gap-1.5 rounded-lg border border-rose-500/40 bg-rose-500/15 px-3 py-1.5 text-xs font-medium text-rose-300 transition hover:bg-rose-500/25 active:scale-[0.985]"
              >
                <IconTrash className="size-3.5" />
                <span>Cancelar Execução</span>
              </button>
            )}
          </div>
        </div>
      )}

      {/* ═══════════════════════════════════════════════
          MODO 3: HISTÓRICO COMPLETO DE JOBS (CONTINUAÇÃO DO ACTION CENTER)
          ═══════════════════════════════════════════════ */}
      {!error && viewMode === "history" && !selectedJob && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <h3 className="font-display text-sm font-semibold text-zinc-200">
              Todos os Jobs do Nó Local ({sorted.length})
            </h3>
            <button
              type="button"
              onClick={() => void fetchJobs()}
              className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-zinc-300 hover:bg-white/10 active:scale-[0.985]"
            >
              <IconRefresh className="size-3.5" />
              <span>Atualizar</span>
            </button>
          </div>

          {loading ? (
            <div className="glass-card rounded-2xl p-10 text-center text-xs text-zinc-500 font-mono">
              Carregando histórico de jobs…
            </div>
          ) : sorted.length === 0 ? (
            <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
              <span className="flex size-10 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
                <IconTarget className="size-5 text-zinc-600" />
              </span>
              <p className="text-xs font-medium text-zinc-300">
                Nenhum job registrado no histórico
              </p>
              <button
                type="button"
                onClick={() => setViewMode("setup")}
                className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-3 py-1.5 text-xs font-medium text-zinc-200 hover:bg-white/10"
              >
                <IconPlay className="size-3 text-brand-400" />
                <span>Iniciar Primeiro Treino</span>
              </button>
            </div>
          ) : (
            <div className="grid grid-cols-1 gap-2.5">
              {sorted.map((job) => {
                const isActive =
                  job.status === "queued" ||
                  job.status === "running" ||
                  job.status === "cancelling";
                const pct = Math.round((job.progress ?? 0) * 100);

                return (
                  <div
                    key={job.id}
                    className="glass-card group flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-xl p-4 transition hover:border-zinc-700"
                  >
                    <div className="flex items-center gap-3 min-w-0 flex-1">
                      <span
                        className={`shrink-0 rounded-full border px-2.5 py-0.5 font-mono text-[10px] font-medium ${STATUS_STYLE[job.status]}`}
                      >
                        {STATUS_LABEL[job.status]}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <span className="text-xs font-semibold text-zinc-100 truncate">
                            {job.model}
                          </span>
                          <span className="font-mono text-[10px] text-zinc-500 truncate">
                            · {job.kind} · {job.engine}
                          </span>
                        </div>
                        <div className="mt-0.5 flex items-center gap-3 font-mono text-[10px] text-zinc-400">
                          <span>{formatRelativeTime(job.createdAt)}</span>
                          <span>Duração: {formatDuration(job.createdAt, job.finishedAt)}</span>
                          {isActive && <span className="text-brand-300">{pct}%</span>}
                        </div>
                      </div>
                    </div>

                    <div className="flex items-center space-x-2 shrink-0">
                      <button
                        type="button"
                        onClick={() => setSelectedJobId(job.id)}
                        className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.06] px-2.5 py-1.5 text-xs font-medium text-zinc-200 transition hover:bg-white/15 active:scale-[0.985]"
                      >
                        <span>Ver detalhes</span>
                        <span>→</span>
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
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

