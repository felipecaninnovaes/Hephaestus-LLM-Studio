"use client";

import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import Link from "next/link";
import {
  abortJob,
  deleteJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  listJobs,
} from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { applyAutolabelCaptions } from "@/lib/autolabel";
import { trainingMetrics } from "@/lib/jobMetrics";
import { jobCapabilities } from "@/lib/jobCapabilities";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import { Button } from "@/components/ui/Button";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import {
  ConvergenceChart,
  MetricSparkline,
} from "@/components/studio/ConvergenceChart";
import { JobSamplesGallery } from "@/components/studio/JobSamplesGallery";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import { useJobTelemetry } from "@/hooks/useJobTelemetry";
import { JobProgressLive } from "@/components/studio/JobProgressLive";
import {
  IconActivity,
  IconCheck,
  IconDatabase,
  IconDownload,
  IconPlay,
  IconRefresh,
  IconSparkles,
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
import { autotrackerErrorMessage, autolabelErrorMessage } from "@/types/studio";
import { formatBytes, formatDuration } from "@/lib/format";
import { openActionCenter } from "@/lib/events";
import { JobListItem } from "@/components/studio/JobCard";
import { AutolabelReviewModal } from "@/components/studio/AutolabelReviewModal";
import { AutotrackerReviewModal } from "@/components/studio/AutotrackerReviewModal";
import { JobCleanupDialog } from "@/components/studio/JobCleanupDialog";

const POLL_INTERVAL = 3000;

const SPARK_COLORS: Record<string, string> = {
  map50: "#34d399",
  map5095: "#2dd4bf",
  boxLoss: "#38bdf8",
  clsLoss: "#818cf8",
  dflLoss: "#fbbf24",
  loss: "#818cf8",
  lr: "#38bdf8",
  step: "#a1a1aa",
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
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
  const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
  const [abortTarget, setAbortTarget] = useState<Job | null>(null);
  const [abortBusy, setAbortBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [applyOverwrite, setApplyOverwrite] = useState(false);
  const [reviewJob, setReviewJob] = useState<Job | null>(null);
  const [autotrackerReviewJob, setAutotrackerReviewJob] = useState<Job | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Job | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [cleanupOpen, setCleanupOpen] = useState(false);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Lê query param ?job=jobId para auto-seleção (deep link / navegação do ActionCenter)
  useEffect(() => {
    const qJob = searchParams.get("job");
    if (qJob && qJob !== selectedJobId) {
      setSelectedJobId(qJob);
    }
  }, [searchParams, selectedJobId]);

  // Estado derivado: modo foco = /jobs?job=ID&focus=1
  const focusMode = searchParams.get("focus") === "1" && Boolean(selectedJobId);

  /** Sincroniza seleção de job com a URL (substitui setSelectedJobId direto). */
  const selectJob = useCallback(
    (id: string | null) => {
      setSelectedJobId(id);
      const params = new URLSearchParams(window.location.search);
      if (id) {
        params.set("job", id);
      } else {
        params.delete("job");
      }
      params.delete("focus"); // sair de foco ao trocar/clear job
      const qs = params.toString();
      router.replace(qs ? `/jobs?${qs}` : "/jobs", { scroll: false });
    },
    [router],
  );

  /** Atualiza o parâmetro ?focus=1 sem trocar o job. */
  const setFocus = useCallback(
    (on: boolean, id?: string | null) => {
      const target = id ?? selectedJobId;
      if (!target) return;
      if (target !== selectedJobId) setSelectedJobId(target);
      const params = new URLSearchParams(window.location.search);
      params.set("job", target);
      if (on) {
        params.set("focus", "1");
      } else {
        params.delete("focus");
      }
      const qs = params.toString();
      router.replace(`/jobs?${qs}`, { scroll: false });
    },
    [router, selectedJobId],
  );

  // Resetar applyOverwrite ao trocar de job
  useEffect(() => {
    setApplyOverwrite(false);
  }, [selectedJobId]);

  const fetchJobs = useCallback(async (isManual = false) => {
    if (isManual) setRefreshing(true);
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
      if (isManual) setRefreshing(false);
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
      return jobs.find((j) => j.id === selectedJobId) ?? activeJobs[0] ?? terminalJobs[0] ?? null;
    }
    // Auto-focus no job ativo mais recente, ou no mais recente terminal
    return activeJobs[0] ?? terminalJobs[0] ?? null;
  }, [jobs, selectedJobId, activeJobs, terminalJobs]);

  const isSelectedActive = selectedJob ? isActive(selectedJob.status) : false;
  const telemetry = useJobTelemetry(isSelectedActive ? selectedJob?.id : null);

  // AC-006-B: pontos reais de métrica de treino do job selecionado
  // (linhas de status/boot do engine ficam só no log, nunca no gráfico/chips)
  const selectedTrainingMetrics = useMemo(
    () => (selectedJob ? trainingMetrics(metrics[selectedJob.id] ?? []) : []),
    [selectedJob, metrics],
  );

  // AC-002: capacidades do job selecionado para gates de UI
  const caps = useMemo(
    () => (selectedJob ? jobCapabilities(selectedJob) : null),
    [selectedJob],
  );

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

  async function handleDeleteJob() {
    if (!deleteTarget) return;
    setDeleteBusy(true);
    try {
      const res = await deleteJob(deleteTarget.id);
      showToast(
        `Job excluído · ${res.artifacts.length} artefatos, ${res.modelsDeleted} modelo(s) do catálogo${res.generationsPreserved > 0 ? ` · ${res.generationsPreserved} geração(ões) da galeria preservadas` : ""}.`,
        "success",
      );
      // Se o job excluído era o selecionado, limpar seleção
      if (selectedJobId === deleteTarget.id) {
        selectJob(null);
      }
      setDeleteTarget(null);
      await fetchJobs();
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "job_not_terminal" || (err as any).status === 409)
      ) {
        showToast("Só jobs concluídos/falhos/cancelados podem ser excluídos.", "info");
        setDeleteTarget(null);
        return;
      }
      showToast("Falha ao excluir job.", "error");
    } finally {
      setDeleteBusy(false);
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

  async function handleApplyCaptions(job: Job) {
    setApplyBusy(true);
    try {
      const result = await applyAutolabelCaptions(job.id, {
        datasetId: job.datasetId,
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} legendas aplicadas, ${result.skipped} ignoradas em ${result.images} imagem(ns).`,
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
        showToast(autolabelErrorMessage(err.code), "error");
        return;
      }
      showToast("Falha ao aplicar legendas.", "error");
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

  function handleResumeFromCheckpoint(job: Job, art: JobArtifact) {
    const checkpointName = art.path.split("/").pop() || "checkpoint.safetensors";
    const match = art.path.match(/epoch_(\d+)/);
    const epochOffset = match ? parseInt(match[1], 10) : (job.epoch ?? 0);

    const resumeData = {
      resumeCheckpoint: {
        id: art.id,
        name: checkpointName,
        epoch: epochOffset,
      },
      epochOffset,
      initialPreset: job.params
        ? {
            baseModel: (job.params.baseModel || (job.params as any).base_model || job.model) as any,
            triggerWord: (job.params.triggerWord ?? (job.params as any).trigger_word ?? "") as string,
            rank: typeof job.params.rank === "number" ? job.params.rank : 16,
            alpha: typeof job.params.alpha === "number" ? job.params.alpha : 16,
            resolution:
              typeof job.params.resolution === "number" ? job.params.resolution : 1024,
            gradientAccumulationSteps:
              typeof job.params.gradientAccumulationSteps === "number"
                ? job.params.gradientAccumulationSteps
                : typeof (job.params as any).gradient_accumulation_steps === "number"
                  ? (job.params as any).gradient_accumulation_steps
                  : 1,
            optimizer: ((job.params.optimizer || (job.params as any).optimizer) as any) || "adamw8bit",
            lrScheduler:
              ((job.params.lrScheduler || (job.params as any).lr_scheduler) as any) || "cosine",
            mixedPrecision:
              ((job.params.mixedPrecision || (job.params as any).mixed_precision) as any) || "fp16",
            quantization:
              ((job.params.quantization || (job.params as any).quantization) as any) || "4bit",
            checkpointInterval:
              typeof job.params.checkpointInterval === "number"
                ? job.params.checkpointInterval
                : typeof (job.params as any).checkpoint_interval === "number"
                  ? (job.params as any).checkpoint_interval
                  : 1,
          }
        : undefined,
    };

    try {
      sessionStorage.setItem("hephaestus_diffusion_resume", JSON.stringify(resumeData));
    } catch {
      // Best-effort
    }

    router.push(
      `/difusao?checkpointId=${art.id}&checkpointName=${encodeURIComponent(checkpointName)}&epochOffset=${epochOffset}`
    );
  }

  function handleRerunJob(job: Job) {
    if (job.engine === "diffusion") {
      const resumeData = {
        datasetId: job.datasetId,
        epochOffset: 0,
        initialPreset: job.params
          ? {
              baseModel: (job.params.baseModel || (job.params as any).base_model || job.model) as any,
              triggerWord: (job.params.triggerWord ?? (job.params as any).trigger_word ?? "") as string,
              rank: typeof job.params.rank === "number" ? job.params.rank : 16,
              alpha: typeof job.params.alpha === "number" ? job.params.alpha : 16,
              resolution:
                typeof job.params.resolution === "number" ? job.params.resolution : 1024,
              gradientAccumulationSteps:
                typeof job.params.gradientAccumulationSteps === "number"
                  ? job.params.gradientAccumulationSteps
                  : typeof (job.params as any).gradient_accumulation_steps === "number"
                    ? (job.params as any).gradient_accumulation_steps
                    : 1,
              optimizer: ((job.params.optimizer || (job.params as any).optimizer) as any) || "adamw8bit",
              lrScheduler:
                ((job.params.lrScheduler || (job.params as any).lr_scheduler) as any) || "cosine",
              mixedPrecision:
                ((job.params.mixedPrecision || (job.params as any).mixed_precision) as any) || "fp16",
              quantization:
                ((job.params.quantization || (job.params as any).quantization) as any) || "4bit",
              checkpointInterval:
                typeof job.params.checkpointInterval === "number"
                  ? job.params.checkpointInterval
                  : typeof (job.params as any).checkpoint_interval === "number"
                    ? (job.params as any).checkpoint_interval
                    : 1,
              epochs:
                typeof job.params.epochs === "number"
                  ? job.params.epochs
                  : typeof (job.params as any).epochs === "number"
                    ? (job.params as any).epochs
                    : 10,
              batchSize:
                typeof job.params.batchSize === "number"
                  ? job.params.batchSize
                  : typeof (job.params as any).batch_size === "number"
                    ? (job.params as any).batch_size
                    : 1,
              learningRate:
                job.params.learningRate != null
                  ? String(job.params.learningRate)
                  : (job.params as any).learning_rate != null
                    ? String((job.params as any).learning_rate)
                    : "0.0001",
              enableSamples:
                job.params.enableSamples ??
                (job.params as any).enable_samples ??
                Boolean(job.params.samplePrompt || (job.params as any).sample_prompt),
              samplePrompt:
                job.params.samplePrompt ?? (job.params as any).sample_prompt ?? "",
              sampleInterval:
                typeof job.params.sampleInterval === "number"
                  ? job.params.sampleInterval
                  : typeof (job.params as any).sample_interval === "number"
                    ? (job.params as any).sample_interval
                    : 1,
              sampleSeed:
                job.params.sampleSeed != null
                  ? String(job.params.sampleSeed)
                  : (job.params as any).sample_seed != null
                    ? String((job.params as any).sample_seed)
                    : "42",
            }
          : undefined,
      };

      try {
        sessionStorage.setItem("hephaestus_diffusion_resume", JSON.stringify(resumeData));
      } catch {
        // Best-effort
      }

      router.push(`/difusao?datasetId=${job.datasetId}`);
    } else {
      router.push(`/treino?datasetId=${job.datasetId}`);
    }
  }

  function handleDownloadJobConfig(job: Job) {
    const jobArts = artifacts[job.id] || [];
    const configArt = jobArts.find(
      (a) => a.kind === "config" || a.path.endsWith("training_config.json")
    );
    if (configArt) {
      handleDownloadArtifact(job.id, configArt);
      return;
    }

    // Fallback gerando direto de job.params ou dados do job
    const configData = job.params || {
      jobId: job.id,
      engine: job.engine,
      model: job.model,
      datasetId: job.datasetId,
      epoch: job.epoch,
      metrics: job.metrics,
      createdAt: job.createdAt,
    };

    const blob = new Blob([JSON.stringify(configData, null, 2)], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `training_config_${job.id.slice(0, 8)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast("Configuração JSON de treino baixada com sucesso.", "success");
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
            onClick={() => setCleanupOpen(true)}
            title="Limpar jobs antigos do histórico"
          >
            <IconTrash className="size-3.5 text-rose-400" />
            <span className="hidden sm:inline">Limpar antigos</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => void fetchJobs(true)}
            disabled={refreshing}
            title="Atualizar lista e status dos jobs"
          >
            <IconRefresh className={`size-3.5 ${refreshing ? "animate-spin text-brand-400" : ""}`} />
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
          {/* Coluna 1: Lista de Execuções — oculta no modo foco */}
          {!focusMode && (
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
                          onSelect={selectJob}
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
                          onSelect={selectJob}
                          onRerun={handleRerunJob}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            )}
          </aside>
          )}

          {/* Coluna 2: Painel de Detalhe (flex-1 min-w-0) */}
          <section className="w-full flex-1 min-w-0 space-y-4">
            {selectedJob ? (
              <div className="space-y-4">
                {/* Cabeçalho do Job Selecionado */}
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <h2 className="font-display text-sm font-semibold text-zinc-200">
                      {focusMode
                        ? "Acompanhando"
                        : isActive(selectedJob.status)
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
                        onClick={() => setFocus(false)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[11px] font-mono text-zinc-300 transition hover:bg-white/[0.06] active:scale-[0.985] cursor-pointer"
                        title="Sair do modo foco"
                      >
                        <IconX className="size-3" />
                        <span>Sair do foco</span>
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => setFocus(true, selectedJob.id)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[11px] font-mono text-zinc-300 transition hover:bg-white/[0.06] active:scale-[0.985] cursor-pointer"
                        title="Acompanhar este job em tela cheia"
                      >
                        <IconActivity className="size-3" />
                        <span>Acompanhar</span>
                      </button>
                    )}
                    {!isActive(selectedJob.status) && (
                      <button
                        type="button"
                        onClick={() => setDeleteTarget(selectedJob)}
                        className="inline-flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 text-[11px] font-mono text-rose-300 transition hover:border-rose-500/40 hover:bg-rose-500/10 active:scale-[0.985] cursor-pointer"
                        aria-label="Excluir job"
                        title="Excluir este job e seus artefatos"
                      >
                        <IconTrash className="size-3" />
                        <span>Excluir</span>
                      </button>
                    )}
                    {selectedJobId && selectedJobId !== activeJobs[0]?.id && (
                      <button
                        type="button"
                        onClick={() => selectJob(null)}
                        className="text-xs text-zinc-400 hover:text-zinc-200 transition underline underline-offset-2"
                      >
                        {activeJobs.length > 0 ? "Voltar ao job ativo" : "Ver último job"}
                      </button>
                    )}
                  </div>
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
                      <div className="mt-1 flex flex-wrap items-center gap-3 font-mono text-[11px] text-zinc-400">
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
                        {selectedJob.orchestratorName ? (
                          <span className="flex items-center gap-1.5 text-zinc-300">
                            <span className="text-zinc-500">·</span>
                            <span>
                              Nó: <strong className="font-semibold text-zinc-200">{selectedJob.orchestratorName}</strong>
                              {selectedJob.orchestratorKind ? ` (${selectedJob.orchestratorKind})` : ""}
                            </span>
                            {selectedJob.orchestratorFallback && (
                              <span
                                className="inline-flex items-center rounded border border-amber-500/30 bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-400 font-medium"
                                title="Job sofreu fallback automático após timeout no nó solicitado"
                              >
                                fallback
                              </span>
                            )}
                          </span>
                        ) : selectedJob.status === "queued" ? (
                          <span className="flex items-center gap-1.5 text-zinc-500">
                            <span>·</span>
                            <span>Aguardando nó</span>
                          </span>
                        ) : null}
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

                  {/* Telemetria Unificada ao Vivo para jobs ativos (ADR-0021) */}
                  {isActive(selectedJob.status) && (
                    <div className="pt-2">
                      <JobProgressLive
                        phase={telemetry.phase || selectedJob.phase || selectedJob.status}
                        phaseMessage={telemetry.phaseMessage || selectedJob.phaseMessage}
                        progress={telemetry.progress || selectedJob.progress || 0}
                        vramUsedGb={telemetry.vramUsedGb ?? selectedJob.vramUsedGb}
                        step={telemetry.step ?? selectedJob.step}
                        epoch={telemetry.epoch ?? selectedJob.epoch}
                        isLive={telemetry.isLive}
                        isFinished={telemetry.isFinished}
                      />
                    </div>
                  )}

                  {/* Métricas da Execução & Curvas de Convergência — regido por caps (AC-002) */}
                  {caps && caps.convergenceChart && selectedTrainingMetrics.length > 0 && (
                    <div className="space-y-4 pt-3 border-t border-white/10">
                      <ConvergenceChart
                        metrics={selectedTrainingMetrics}
                        totalEpochs={selectedJob.epoch || 100}
                        isJobActive={selectedJob.status === "running"}
                      />

                      {selectedTrainingMetrics.length > 0 && (
                        <div className="space-y-2">
                          <div className="flex items-center justify-between">
                            <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                              Métricas (Epoch{" "}
                              {selectedTrainingMetrics[selectedTrainingMetrics.length - 1].epoch}
                              )
                            </h3>
                            <span className="font-mono text-[11px] text-zinc-400">
                              {selectedTrainingMetrics.length} checkpoint(s)
                            </span>
                          </div>
                          <div className={`grid gap-2.5 ${
                            caps.metricChips === "diffusion"
                              ? "grid-cols-2 sm:grid-cols-4"
                              : "grid-cols-2 sm:grid-cols-3 lg:grid-cols-6"
                          }`}>
                            {(
                              caps.metricChips === "diffusion"
                                ? ([
                                    ["loss", "Diffusion Loss", false],
                                    ["lr", "Learning Rate", false],
                                    ["step", "Step", false],
                                    ["epoch", "Época", false],
                                  ] as const)
                                : ([
                                    ["map50", "mAP@50", true],
                                    ["map5095", "mAP@50-95", true],
                                    ["boxLoss", "Box Loss", false],
                                    ["clsLoss", "Cls Loss", false],
                                    ["dflLoss", "Dfl Loss", false],
                                    ["epoch", "Epochs", false],
                                  ] as const)
                            ).map(([key, label, isPercent]) => {
                              const jobMetrics = selectedTrainingMetrics;
                              const last = jobMetrics[jobMetrics.length - 1];
                              const val = last[key as keyof JobMetricsType];
                              const isPrimary = key === "map50" || key === "loss";
                              const series = jobMetrics.map(
                                (m) => (m[key as keyof JobMetricsType] as number) ?? 0,
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
                                          : key === "lr"
                                            ? val.toExponential(2)
                                            : key === "epoch" || key === "step"
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

                  {/* Chip único "Imagens processadas" para autolabel/autotracker (AC-002) */}
                  {caps && caps.metricChips === "progress" && (
                    <div className="pt-3 border-t border-white/10">
                      <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 inline-flex items-center gap-2">
                        <span className="text-[10px] font-mono text-zinc-400 uppercase tracking-caps">Imagens processadas</span>
                        <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                          {(() => {
                            const processed = selectedJob.step ?? (selectedJob.progress != null ? Math.round(selectedJob.progress * (selectedJob.step ?? 0)) : null);
                            if (processed != null) {
                              return `${processed}${selectedJob.step ? `/${selectedJob.step}` : ""}`;
                            }
                            return selectedJob.progress != null ? `${Math.round(selectedJob.progress * 100)}%` : "—";
                          })()}
                        </span>
                      </div>
                    </div>
                  )}

                  {/* Artefatos e Amostras Geradas */}
                  {artifacts[selectedJob.id] && artifacts[selectedJob.id].length > 0 && (
                    <div className="space-y-4 pt-3 border-t border-white/10">
                      {/* Galeria de Amostras — gated por caps.samplesGallery (AC-002) */}
                      {caps && caps.samplesGallery && (
                        <JobSamplesGallery
                          jobId={selectedJob.id}
                          artifacts={artifacts[selectedJob.id]}
                          onDownload={(jId, art) => handleDownloadArtifact(jId, art)}
                        />
                      )}

                      {/* Outros Artefatos Gerados */}
                      {artifacts[selectedJob.id].filter(
                        (art) =>
                          art.kind !== "sample" &&
                          !art.path.startsWith("samples/") &&
                          !art.path.includes("sample_epoch_"),
                      ).length > 0 && (
                        <div className="space-y-2">
                          <h3 className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-300">
                            Artefatos ({artifacts[selectedJob.id].filter(
                              (art) =>
                                art.kind !== "sample" &&
                                !art.path.startsWith("samples/") &&
                                !art.path.includes("sample_epoch_"),
                            ).length})
                          </h3>
                          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2.5">
                            {artifacts[selectedJob.id]
                              .filter(
                                (art) =>
                                  art.kind !== "sample" &&
                                  !art.path.startsWith("samples/") &&
                                  !art.path.includes("sample_epoch_"),
                              )
                              .map((art) => (
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
                                    <div className="flex items-center gap-2 shrink-0">
                                      {(art.kind === "checkpoint" ||
                                        art.kind === "model" ||
                                        art.path.endsWith(".safetensors")) && (
                                        <Button
                                          type="button"
                                          variant="secondary"
                                          size="sm"
                                          onClick={() =>
                                            handleResumeFromCheckpoint(selectedJob, art)
                                          }
                                          title="Retomar treino a partir deste checkpoint"
                                        >
                                          <IconSparkles className="size-3.5 text-sky-400" />
                                          <span>Retomar</span>
                                        </Button>
                                      )}
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
                                </div>
                              ))}
                          </div>
                        </div>
                      )}
                    </div>
                  )}

                    {/* Ações do Job */}
                    <div className="flex items-center justify-between gap-3 pt-2">
                    {caps && caps.applyAction && selectedJob.kind === "autotracker" && selectedJob.status === "done" && (
                      <div className="flex items-center gap-3 flex-wrap">
                        <Button
                          type="button"
                          variant="primary"
                          size="sm"
                          onClick={() => setAutotrackerReviewJob(selectedJob)}
                        >
                          <IconSparkles className="size-3.5 text-brand-400" />
                          <span>Revisar e Aplicar</span>
                        </Button>

                        <div className="flex items-center gap-2 border-l border-white/10 pl-3">
                          <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
                            <input
                              type="checkbox"
                              checked={applyOverwrite}
                              onChange={(e) => setApplyOverwrite(e.target.checked)}
                              className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40"
                            />
                            <span>Sobrescrever</span>
                          </label>
                          <Button
                            type="button"
                            variant="secondary"
                            size="sm"
                            disabled={applyBusy}
                            loading={applyBusy}
                            onClick={() => handleApplyBoxes(selectedJob)}
                          >
                            <IconCheck className="size-3.5 text-zinc-300" />
                            <span>{applyBusy ? "Aplicando…" : "Aplicação direta"}</span>
                          </Button>
                        </div>
                      </div>
                    )}

                    {caps && caps.applyAction && selectedJob.kind === "autolabel" && selectedJob.status === "done" && (
                      <div className="flex items-center gap-3 flex-wrap">
                        <Button
                          type="button"
                          variant="primary"
                          size="sm"
                          onClick={() => setReviewJob(selectedJob)}
                        >
                          <IconSparkles className="size-3.5 text-brand-400" />
                          <span>Revisar Legendas (Curadoria)</span>
                        </Button>

                        <div className="flex items-center gap-2 border-l border-white/10 pl-3">
                          <label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
                            <input
                              type="checkbox"
                              checked={applyOverwrite}
                              onChange={(e) => setApplyOverwrite(e.target.checked)}
                              className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40"
                            />
                            <span>Sobrescrever</span>
                          </label>
                          <Button
                            type="button"
                            variant="secondary"
                            size="sm"
                            disabled={applyBusy}
                            loading={applyBusy}
                            onClick={() => handleApplyCaptions(selectedJob)}
                            title="Aplica todas as legendas geradas sem inspeção prévia"
                          >
                            <IconCheck className="size-3.5 text-zinc-400" />
                            <span>{applyBusy ? "Aplicando…" : "Aplicar Todas Direto"}</span>
                          </Button>
                        </div>
                      </div>
                    )}

                    {/* Ações para jobs finalizados de difusão ou YOLO — gated por caps.rerun (AC-002) */}
                    {caps && caps.rerun && !isActive(selectedJob.status) &&
                      (selectedJob.engine === "diffusion" || selectedJob.engine === "yolo") && (
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => handleRerunJob(selectedJob)}
                          title="Abrir a Forja pré-carregada com todos os parâmetros deste treino para submeter novamente"
                        >
                          <IconRefresh className="size-3.5 text-brand-400" />
                          <span>Repetir Treino</span>
                        </Button>
                      )}

                    {selectedJob.engine === "diffusion" && (
                      <div className="flex items-center gap-2.5 flex-wrap">
                        <Button
                          type="button"
                          variant="secondary"
                          size="sm"
                          onClick={() => handleDownloadJobConfig(selectedJob)}
                          title="Baixar JSON com os parâmetros de configuração deste treino"
                        >
                          <IconDownload className="size-3.5 text-zinc-400" />
                          <span>Baixar JSON de Treino</span>
                        </Button>

                        {artifacts[selectedJob.id] &&
                          artifacts[selectedJob.id].some(
                            (a) =>
                              a.kind === "checkpoint" ||
                              a.kind === "model" ||
                              a.path.endsWith(".safetensors")
                          ) && (
                            <Button
                              type="button"
                              variant="primary"
                              size="sm"
                              onClick={() => {
                                const ckpts = (artifacts[selectedJob.id] || []).filter(
                                  (a) =>
                                    a.kind === "checkpoint" ||
                                    a.kind === "model" ||
                                    a.path.endsWith(".safetensors")
                                );
                                const lastCkpt = ckpts[ckpts.length - 1];
                                if (lastCkpt) {
                                  handleResumeFromCheckpoint(selectedJob, lastCkpt);
                                }
                              }}
                              title="Continuar treinamento adicionando épocas a partir do último checkpoint"
                            >
                              <IconSparkles className="size-3.5 text-sky-400" />
                              <span>Continuar Treino</span>
                            </Button>
                          )}
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

      {/* Confirmação de exclusão de Job */}
      <ConfirmDialog
        open={Boolean(deleteTarget)}
        title="Excluir job"
        body={
          <p className="text-xs text-zinc-300">
            Esta ação remove o job{" "}
            <strong className="text-white font-mono">{deleteTarget?.model}</strong>{" "}
            ({deleteTarget?.id.slice(0, 8)}…) do histórico e{" "}
            <strong>apaga seus artefatos no armazenamento</strong>. Modelos derivados
            deste job saem do catálogo. As imagens já salvas na galeria de geração
            são preservadas.
          </p>
        }
        confirmLabel="Sim, excluir job"
        danger
        busy={deleteBusy}
        onConfirm={handleDeleteJob}
        onClose={() => setDeleteTarget(null)}
      />

      {/* Diálogo de limpeza em lote */}
      <JobCleanupDialog
        open={cleanupOpen}
        onClose={() => setCleanupOpen(false)}
        terminalJobs={terminalJobs.map((j) => ({
          id: j.id,
          status: j.status as "done" | "failed" | "cancelled",
          createdAt: j.createdAt,
          finishedAt: j.finishedAt,
        }))}
        onDone={() => void fetchJobs()}
      />

      <AutolabelReviewModal
        open={!!reviewJob}
        onClose={() => setReviewJob(null)}
        jobId={reviewJob?.id ?? null}
        datasetId={reviewJob?.datasetId}
        onApplied={() => {
          void fetchJobs();
        }}
      />

      {autotrackerReviewJob && (
        <AutotrackerReviewModal
          open={!!autotrackerReviewJob}
          job={autotrackerReviewJob}
          onClose={() => setAutotrackerReviewJob(null)}
          onApplied={() => {
            void fetchJobs();
          }}
        />
      )}
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
