"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
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
  IconDownload,
  IconRefresh,
  IconTarget,
  IconTrash,
} from "@/components/icons";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
} from "@/types/studio";
import { autotrackerErrorMessage } from "@/types/studio";

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

function relativeTime(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return "agora";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m atrás`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h atrás`;
  const d = Math.floor(h / 24);
  return `${d}d atrás`;
}

export default function JobsPage() {
  const router = useRouter();
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
      setError("Falha ao carregar jobs.");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  // Polling: ativo quando há jobs running/queued/cancelling
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

  // Re-fetch métricas quando o job selecionado muda de estado
  useEffect(() => {
    if (!selectedJobId) return;
    const job = jobs.find((j) => j.id === selectedJobId);
    if (!job) return;

    const isActive =
      job.status === "queued" ||
      job.status === "running" ||
      job.status === "cancelling";
    const isTerminal =
      job.status === "done" ||
      job.status === "failed" ||
      job.status === "cancelled";
    if (!isActive && !isTerminal) return;

    const ctrl = new AbortController();

    async function refreshDetail() {
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
        // Ignora
      }
    }
    refreshDetail();
    return () => ctrl.abort();
  }, [selectedJobId, jobs]);

  async function handleAbort() {
    if (!abortTarget) return;
    setAbortBusy(true);
    try {
      await abortJob(abortTarget.id);
      showToast("Job cancelado.", "success");
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
  }

  const sorted = [...jobs].sort(
    (a, b) => new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
  );

  const selectedJob = selectedJobId
    ? jobs.find((j) => j.id === selectedJobId) ?? null
    : null;

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6 md:flex-row md:gap-5">
      {/* ═══════════════════════════════════════════════
          COLUNA ESQUERDA — Activity Feed (≥ md)
          ═══════════════════════════════════════════════ */}
      <aside className="md:w-[300px] md:shrink-0">
        {/* Header do feed — sempre visível */}
        <div className="mb-3 flex items-center justify-between">
          <h2 className="font-display text-sm font-semibold text-zinc-200">
            Atividade
          </h2>
          <button
            type="button"
            onClick={() => void fetchJobs()}
            title="Atualizar lista"
            className="inline-flex h-8 items-center justify-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.05] px-2.5 text-[11px] font-medium text-zinc-300 transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-3.5 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconRefresh className="h-3.5 w-3.5" />
            <span>Atualizar</span>
          </button>
        </div>

        {/* Feed container — overflow-y-auto PRÓPRIO apenas em ≥ md */}
        <div className="flex flex-col gap-1.5 md:h-[calc(100vh-12rem)] md:overflow-y-auto md:rounded-2xl md:border md:border-zinc-800/80 md:bg-black/20 md:p-2">
          {/* Loading */}
          {loading && (
            <p className="py-8 text-center font-mono text-xs text-zinc-500">
              Carregando…
            </p>
          )}

          {/* Empty state */}
          {!loading && sorted.length === 0 && (
            <div className="flex flex-col items-center gap-2 py-8 text-center">
              <span className="flex h-10 w-10 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900">
                <IconTarget className="h-5 w-5 text-zinc-700" />
              </span>
              <p className="text-xs font-medium text-zinc-400">
                Nenhum treino ainda
              </p>
            </div>
          )}

          {/* Job feed items */}
          {!loading &&
            sorted.map((job) => {
              const isSelected = selectedJobId === job.id;
              const isActive =
                job.status === "queued" || job.status === "running";
              const pct = Math.round((job.progress ?? 0) * 100);

              return (
                <button
                  key={job.id}
                  type="button"
                  onClick={() => setSelectedJobId(job.id)}
                  className={`flex w-full flex-col gap-1 rounded-xl p-3 text-left transition-all ${
                    isSelected
                      ? "border border-brand-500/30 bg-brand-500/[0.12]"
                      : "border border-transparent hover:bg-white/[0.04]"
                  }`}
                >
                  {/* Top row: status + time */}
                  <div className="flex items-center justify-between gap-2">
                    <span
                      className={`shrink-0 rounded-full border px-2 py-0.5 font-mono text-[10px] font-medium ${STATUS_STYLE[job.status]}`}
                    >
                      {STATUS_LABEL[job.status]}
                    </span>
                    <span
                      className="font-mono text-[10px] text-zinc-500"
                      title={new Date(job.createdAt).toLocaleString("pt-BR")}
                    >
                      {relativeTime(job.createdAt)}
                    </span>
                  </div>

                  {/* Model + kind */}
                  <div className="min-w-0">
                    <span className="block truncate text-xs font-semibold text-zinc-100">
                      {job.model}
                    </span>
                    <span className="block truncate font-mono text-[10px] text-zinc-500">
                      {job.kind}
                    </span>
                  </div>

                  {/* Progress bar (active jobs) */}
                  {isActive && (
                    <div className="h-1 w-full overflow-hidden rounded-full bg-zinc-800">
                      <div
                        className="h-full rounded-full bg-[#34d399] transition-all duration-500"
                        style={{ width: `${pct}%` }}
                      />
                    </div>
                  )}

                  {/* Cancelling indicator */}
                  {job.status === "cancelling" && (
                    <span className="font-mono text-[10px] text-zinc-500">
                      cancelando…
                    </span>
                  )}
                </button>
              );
            })}
        </div>
      </aside>

      {/* ═══════════════════════════════════════════════
          ÁREA CENTRAL — Setup ou Detalhe do Job
          ═══════════════════════════════════════════════ */}
      <main className="min-w-0 flex-1">
        {/* Error global */}
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

        {/* Estado: nenhum job selecionado → Setup */}
        {!error && !selectedJob && <ForjaYoloSetup onJobCreated={handleJobCreated} />}

        {/* Estado: job selecionado → Detalhe */}
        {!error && selectedJob && (
          <div className="space-y-5">
            {/* Voltar ao setup */}
            <button
              type="button"
              onClick={() => setSelectedJobId(null)}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-300 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              ← Voltar ao setup
            </button>

            {/* Job header */}
            <div className="glass-card rounded-2xl p-5">
              <div className="flex items-center gap-3">
                <span
                  className={`shrink-0 rounded-full border px-2.5 py-1 font-mono text-[11px] font-medium ${STATUS_STYLE[selectedJob.status]}`}
                >
                  {STATUS_LABEL[selectedJob.status]}
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-semibold text-zinc-100">
                      {selectedJob.model}
                    </span>
                    <span className="font-mono text-[10px] text-zinc-500">
                      {selectedJob.kind}
                    </span>
                    {selectedJob.queuePosition != null &&
                      selectedJob.status === "queued" && (
                        <span className="font-mono text-[10px] text-zinc-500">
                          #{selectedJob.queuePosition}
                        </span>
                      )}
                  </div>
                  <div className="mt-0.5 flex items-center gap-3 font-mono text-[10px] text-zinc-400">
                    <span title={`Criado em ${new Date(selectedJob.createdAt).toLocaleString("pt-BR")}`}>
                      {formatDuration(selectedJob.createdAt, selectedJob.finishedAt)}
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
                <span className="font-mono text-[10px] text-zinc-500" title={selectedJob.id}>
                  {selectedJob.id.slice(0, 8)}
                </span>
              </div>

              {/* Progress bar full width */}
              {(selectedJob.status === "queued" ||
                selectedJob.status === "running") && (
                <div className="mt-3 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                  <div
                    className="h-full rounded-full bg-[#34d399] transition-all duration-500"
                    style={{
                      width: `${Math.round((selectedJob.progress ?? 0) * 100)}%`,
                    }}
                  />
                </div>
              )}
            </div>

            {/* Métricas */}
            {metrics[selectedJob.id] && metrics[selectedJob.id].length > 0 && (
              <div className="glass-card rounded-2xl p-5">
                <h4 className="tracking-caps mb-3 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                  {"Métricas (Epoch "}
                  {metrics[selectedJob.id]![
                    metrics[selectedJob.id]!.length - 1
                  ].epoch}
                  {")"}
                </h4>
                <div className="grid grid-cols-3 gap-2 sm:grid-cols-6">
                  {(
                    [
                      ["boxLoss", "Box Loss"],
                      ["clsLoss", "Cls Loss"],
                      ["dflLoss", "Dfl Loss"],
                      ["map50", "mAP@50"],
                      ["map5095", "mAP@50-95"],
                      ["epoch", "Epoch"],
                    ] as const
                  ).map(([key, label]) => {
                    const jobMetrics = metrics[selectedJob.id]!;
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
            {artifacts[selectedJob.id] &&
              artifacts[selectedJob.id].length > 0 && (
                <div className="glass-card rounded-2xl p-5">
                  <h4 className="tracking-caps mb-3 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                    Artefatos
                  </h4>
                  <div className="flex flex-wrap gap-2">
                    {(artifacts[selectedJob.id] ?? []).map((art) => (
                      <button
                        key={art.id}
                        type="button"
                        onClick={() =>
                          handleDownloadArtifact(selectedJob.id, art)
                        }
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

            {/* Sem artefatos */}
            {selectedJob.status === "done" &&
              artifacts[selectedJob.id] &&
              artifacts[selectedJob.id].length === 0 && (
                <p className="text-xs text-zinc-500">Sem artefatos.</p>
              )}

            {/* Ações */}
            {(selectedJob.status === "queued" ||
              selectedJob.status === "running") && (
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setAbortTarget(selectedJob)}
                  className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.12] px-4 text-xs font-medium whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                >
                  <IconTrash className="h-3.5 w-3.5" />
                  Abortar
                </button>
              </div>
            )}

            {/* Aplicar boxes — somente para autotracker done */}
            {selectedJob.status === "done" &&
              selectedJob.engine === "autotracker" && (
                <div className="glass-card rounded-2xl p-5">
                  <h4 className="tracking-caps mb-3 font-mono text-[11px] font-semibold uppercase text-zinc-400">
                    Aplicar ao dataset
                  </h4>
                  <p className="mb-3 text-xs text-zinc-400">
                    As boxes geradas pelo AutoTracker estão prontas para serem
                    aplicadas ao dataset.
                  </p>
                  <div className="flex items-center gap-3">
                    <label className="flex items-center gap-2 text-xs text-zinc-300">
                      <input
                        type="checkbox"
                        checked={applyOverwrite}
                        onChange={(e) => setApplyOverwrite(e.target.checked)}
                        disabled={applyBusy}
                        className="h-4 w-4 rounded border-zinc-700 bg-black/40 accent-brand-500"
                      />
                      Sobrescrever anotações manuais
                    </label>
                    <button
                      type="button"
                      onClick={() => void handleApplyBoxes(selectedJob)}
                      disabled={applyBusy}
                      className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                    >
                      <IconTarget className="h-3.5 w-3.5" />
                      {applyBusy ? "Aplicando…" : "Aplicar boxes ao dataset"}
                    </button>
                  </div>
                </div>
              )}
          </div>
        )}
      </main>

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
