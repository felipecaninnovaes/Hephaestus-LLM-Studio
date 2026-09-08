"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  abortJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  listJobs,
} from "@/lib/jobs";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import {
  IconDownload,
  IconPlay,
  IconRefresh,
  IconTarget,
  IconTrash,
  IconX,
} from "@/components/icons";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
} from "@/types/studio";

const POLL_INTERVAL = 3000;

const STATUS_STYLE: Record<JobStatus, string> = {
  queued:
    "border-brand-500/30 bg-brand-500/10 text-brand-300",
  running:
    "border-brand-500/30 bg-brand-500/10 text-brand-300",
  cancelling:
    "border-brand-500/30 bg-brand-500/10 text-brand-300",
  done:
    "border-[#34d399]/30 bg-[#34d399]/10 text-[#a7f3d0]",
  failed:
    "border-rose-500/30 bg-rose-500/10 text-rose-300",
  cancelled:
    "border-rose-500/30 bg-rose-500/10 text-rose-300",
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
  const ms = (end ? new Date(end) : new Date()).getTime() - new Date(start).getTime();
  if (ms < 0) return "—";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${s % 60}s`;
}

export default function JobsPage() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
  const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
  const [abortTarget, setAbortTarget] = useState<Job | null>(null);
  const [abortBusy, setAbortBusy] = useState(false);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const pollActiveRef = useRef(true);

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
        // Redireciona será feito pelo proxy/parent
        return;
      }
      setError("Falha ao carregar jobs.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  // Polling: ativo quando há jobs running/queued; pausa quando tudo done/failed/cancelled
  useEffect(() => {
    function hasActiveJobs(list: Job[]) {
      return list.some((j) => j.status === "queued" || j.status === "running" || j.status === "cancelling");
    }

    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }

    if (hasActiveJobs(jobs)) {
      pollActiveRef.current = true;
      pollRef.current = setInterval(() => {
        void fetchJobs();
      }, POLL_INTERVAL);
    } else {
      pollActiveRef.current = false;
    }

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [jobs, fetchJobs]);

  // Carregar métricas e artefatos quando expandir
  useEffect(() => {
    if (!expandedId) return;
    const ctrl = new AbortController();

    async function loadDetail() {
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
        // Detalhe é best-effort — mantém estado atual
      }
    }

    loadDetail();
    return () => ctrl.abort();
  }, [expandedId]);

  // Re-fetch métricas quando um job muda de estado (ativa e terminal)
  useEffect(() => {
    if (!expandedId) return;
    const job = jobs.find((j) => j.id === expandedId);
    if (!job) return;

    const isActive = job.status === "queued" || job.status === "running" || job.status === "cancelling";
    const isTerminal = job.status === "done" || job.status === "failed" || job.status === "cancelled";
    if (!isActive && !isTerminal) return;

    const ctrl = new AbortController();

    async function refreshDetail() {
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
        // Ignora
      }
    }
    refreshDetail();
    return () => ctrl.abort();
  }, [expandedId, jobs]);

  async function handleAbort() {
    if (!abortTarget) return;
    setAbortBusy(true);
    try {
      await abortJob(abortTarget.id);
      showToast("Job cancelado.", "success");
      setAbortTarget(null);
      await fetchJobs();
    } catch (err) {
      if (err instanceof ApiError && (err.code === "job_not_abortable" || err.status === 409)) {
        showToast("Este job não pode mais ser cancelado.", "info");
        setAbortTarget(null);
        return;
      }
      showToast("Falha ao cancelar job.", "error");
    } finally {
      setAbortBusy(false);
    }
  }

  async function handleDownloadArtifact(jobId: string, art: JobArtifact) {
    try {
      await downloadArtifact(jobId, art.id, art.path);
    } catch {
      showToast("Falha ao baixar artefato.", "error");
    }
  }

  const sorted = [...jobs].sort(
    (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
  );

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-baseline gap-3">
          <h1 className="font-display tracking-display truncate text-xl font-semibold text-zinc-100 lg:text-2xl">
            Forja &amp; Treinamento
          </h1>
          <span className="shrink-0 font-mono text-xs text-zinc-500">
            {jobs.length} jobs
          </span>
        </div>
        <button
          type="button"
          onClick={() => {
            void fetchJobs();
          }}
          title="Atualizar lista"
          className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
        >
          <IconRefresh className="h-4 w-4" />
          <span>Atualizar</span>
        </button>
      </div>

      {/* Loading */}
      {loading && (
        <p className="py-10 text-center font-mono text-xs text-zinc-500">
          Carregando jobs…
        </p>
      )}

      {/* Error */}
      {error && (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error}</p>
          <button
            type="button"
            onClick={() => {
              setLoading(true);
              void fetchJobs();
            }}
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            Tentar novamente
          </button>
        </div>
      )}

      {/* Empty */}
      {!loading && !error && sorted.length === 0 && (
        <div className="glass-card flex flex-col items-center gap-2 rounded-2xl p-12 text-center">
          <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
            <IconTarget className="h-6 w-6 text-zinc-700" />
          </span>
          <p className="text-sm font-medium text-zinc-200">
            Nenhum job de treino ainda
          </p>
          <p className="text-xs text-zinc-500">
            Crie um treino YOLO a partir de um dataset para começar.
          </p>
        </div>
      )}

      {/* Job list */}
      {!loading && !error && sorted.length > 0 && (
        <div className="flex flex-col gap-2">
          {sorted.map((job) => {
            const isExpanded = expandedId === job.id;
            const isActive = job.status === "queued" || job.status === "running";
            const pct = Math.round((job.progress ?? 0) * 100);
            const jobMetrics = metrics[job.id] ?? [];
            const jobArtifacts = artifacts[job.id] ?? [];

            return (
              <div
                key={job.id}
                className="glass-card rounded-2xl transition-all"
              >
                {/* Main row */}
                <button
                  type="button"
                  onClick={() => setExpandedId(isExpanded ? null : job.id)}
                  aria-expanded={isExpanded}
                  className="flex w-full items-center gap-3 p-4 text-left"
                >
                  {/* Status pill */}
                  <span
                    className={`shrink-0 rounded-full border px-2.5 py-1 font-mono text-[11px] font-medium ${STATUS_STYLE[job.status]}`}
                  >
                    {STATUS_LABEL[job.status]}
                  </span>

                  {/* Info */}
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-semibold text-zinc-100">
                        {job.model}
                      </span>
                      <span className="font-mono text-[10px] text-zinc-500">
                        {job.kind}
                      </span>
                      {job.queuePosition != null && job.status === "queued" && (
                        <span className="font-mono text-[10px] text-zinc-500">
                          #{job.queuePosition}
                        </span>
                      )}
                    </div>
                    <div className="mt-0.5 flex items-center gap-3 font-mono text-[10px] text-zinc-400">
                      <span title={`Criado em ${new Date(job.createdAt).toLocaleString("pt-BR")}`}>
                        {formatDuration(job.createdAt, job.finishedAt)}
                      </span>
                      {job.epoch != null && (
                        <span>
                          Epoch {job.epoch}
                        </span>
                      )}
                      {isActive && (
                        <span className="text-brand-300">
                          {pct}%
                        </span>
                      )}
                    </div>
                  </div>

                  {/* Progress bar */}
                  {isActive && (
                    <div className="hidden w-32 shrink-0 sm:block">
                      <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                        <div
                          className="h-full rounded-full bg-[#34d399] transition-all duration-500"
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    </div>
                  )}

                  {/* Arrow */}
                  <svg
                    className={`h-4 w-4 shrink-0 text-zinc-500 transition-transform ${isExpanded ? "rotate-180" : ""}`}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.7"
                    viewBox="0 0 24 24"
                  >
                    <polyline points="6 9 12 15 18 9" />
                  </svg>
                </button>

                {/* Expanded detail */}
                {isExpanded && (
                  <div className="border-t border-white/10 px-4 pb-4 pt-3">
                    {/* Métricas (Monospace Truth) */}
                    {jobMetrics.length > 0 && (
                      <div className="mb-4">
                        <h4 className="tracking-caps mb-2 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                          Últimas Métricas (Epoch {jobMetrics[jobMetrics.length - 1].epoch})
                        </h4>
                        <div className="grid grid-cols-3 gap-2 sm:grid-cols-6">
                          {([
                            ["boxLoss", "Box Loss"],
                            ["clsLoss", "Cls Loss"],
                            ["dflLoss", "Dfl Loss"],
                            ["map50", "mAP@50"],
                            ["map5095", "mAP@50-95"],
                            ["epoch", "Epoch"],
                          ] as const).map(([key, label]) => {
                            const last = jobMetrics[jobMetrics.length - 1];
                            const val = last[key];
                            return (
                              <div
                                key={key}
                                className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-2"
                              >
                                <span className="block font-mono text-[10px] text-zinc-500">
                                  {label}
                                </span>
                                <span className="block font-mono text-sm font-medium text-zinc-200">
                                  {typeof val === "number"
                                    ? key === "epoch"
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
                    {jobArtifacts.length > 0 && (
                      <div className="mb-4">
                        <h4 className="tracking-caps mb-2 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                          Artefatos
                        </h4>
                        <div className="flex flex-wrap gap-2">
                          {jobArtifacts.map((art) => (
                            <button
                              key={art.id}
                              type="button"
                              onClick={() => handleDownloadArtifact(job.id, art)}
                              title={`Baixar ${art.path} (${art.bytes} bytes)`}
                              className="inline-flex h-9 items-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                            >
                              <IconDownload className="h-3.5 w-3.5" />
                              <span className="font-mono">{art.path}</span>
                            </button>
                          ))}
                        </div>
                      </div>
                    )}

                    {/* Ações */}
                    <div className="flex items-center gap-2">
                      {(job.status === "queued" || job.status === "running") && (
                        <button
                          type="button"
                          onClick={() => setAbortTarget(job)}
                          className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.12] px-4 text-xs font-medium whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                        >
                          <IconTrash className="h-3.5 w-3.5" />
                          Abortar
                        </button>
                      )}
                      {job.status === "done" && jobArtifacts.length === 0 && (
                        <span className="text-xs text-zinc-500">
                          Sem artefatos
                        </span>
                      )}
                      <span className="ml-auto font-mono text-[10px] text-zinc-500" title={job.id}>
                        {job.id.slice(0, 8)}
                      </span>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Abort confirm dialog */}
      <ConfirmDialog
        open={abortTarget !== null}
        title="Abortar job"
        body={
          abortTarget ? (
            <p>
              Tem certeza que deseja abortar o job{" "}
              <strong className="text-zinc-100">{abortTarget.model}</strong>?
              {abortTarget.status === "queued"
                ? " O job ainda não iniciou."
                : " O job em execução será interrompido."}
            </p>
          ) : null
        }
        confirmLabel="Abortar"
        danger
        busy={abortBusy}
        onConfirm={handleAbort}
        onClose={() => {
          if (!abortBusy) setAbortTarget(null);
        }}
      />
    </div>
  );
}
