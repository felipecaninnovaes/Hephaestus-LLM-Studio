"use client";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconActivity,
  IconAlertTriangle,
  IconBell,
  IconCheck,
  IconChevronDown,
  IconCpu,
  IconDatabase,
  IconDownload,
  IconHardDrive,
  IconInfo,
  IconLayers,
  IconPlay,
  IconRefresh,
  IconServer,
  IconSparkles,
  IconTarget,
  IconTrash,
  IconX,
  IconZap,
} from "@/components/icons";
import { SearchInput, SubmodulePills, Badge, ProgressBar, Drawer, jobStatusToBadgeVariant } from "@/components/ui";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import { AutolabelReviewModal } from "@/components/studio/AutolabelReviewModal";
import { AutotrackerReviewModal } from "@/components/studio/AutotrackerReviewModal";
import {
  abortJob,
  deleteJob,
  downloadArtifact,
  getJobArtifacts,
  getJobMetrics,
  getTelemetry,
  listJobs,
} from "@/lib/jobs";
import { applyAutotrackerBoxes } from "@/lib/autotracker";
import { applyAutolabelCaptions } from "@/lib/autolabel";
import { latestTrainingMetric } from "@/lib/jobMetrics";
import { jobCapabilities, imageProgressLabel } from "@/lib/jobCapabilities";
import { ApiError } from "@/lib/api";
import { copyToClipboard } from "@/lib/clipboard";
import { formatBytes, formatDuration, formatRelativeTime } from "@/lib/format";
import { autotrackerErrorMessage, autolabelErrorMessage } from "@/types/studio";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
  JobStatus,
  Telemetry,
} from "@/types/studio";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import { JobCleanupDialog } from "@/components/studio/JobCleanupDialog";
import { JOB_STATUS_CONFIG, JobArtifactsList } from "./JobCard";
import { JobSamplesGallery } from "./JobSamplesGallery";
import { showToast } from "./Toast";

export interface SystemNotification {
  id: string;
  title: string;
  message: string;
  category: "infra" | "dataset" | "model" | "orchestrator";
  level: "info" | "warning" | "success" | "error";
  timestamp: string;
  actionLabel?: string;
  actionHref?: string;
}

interface ActionCenterProps {
  open: boolean;
  onClose: () => void;
}

type TabFilter = "all" | "active" | "jobs" | "system";

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

  // Busca lista de jobs e telemetria do nó em paralelo
  const fetchData = useCallback(async () => {
    try {
      setLoading(true);
      const [resJobs, resTel] = await Promise.allSettled([
        listJobs(),
        getTelemetry(),
      ]);
      if (resJobs.status === "fulfilled") {
        setJobs(resJobs.value.items);
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
      if (pollRef.current) {
        clearInterval(pollRef.current);
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

    pollRef.current = setInterval(tick, 3000);

    function onVisibilityChange() {
      if (document.visibilityState === "visible") {
        void fetchData();
        if (!pollRef.current) {
          pollRef.current = setInterval(tick, 3000);
        }
      } else {
        if (pollRef.current) {
          clearInterval(pollRef.current);
          pollRef.current = null;
        }
      }
    }

    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      if (pollRef.current) {
        clearInterval(pollRef.current);
        pollRef.current = null;
      }
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [open, fetchData]);

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
      await fetchData();
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

  // Ação de excluir job terminal
  async function handleDeleteJob() {
    if (!deleteTarget) return;
    setDeleteBusy(true);
    try {
      const res = await deleteJob(deleteTarget.id);
      showToast(
        `Job excluído · ${res.artifacts.length} artefatos, ${res.modelsDeleted} modelo(s) do catálogo${res.generationsPreserved > 0 ? ` · ${res.generationsPreserved} geração(ões) da galeria preservadas` : ""}.`,
        "success",
      );
      setDeleteTarget(null);
      await fetchData();
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "job_not_terminal" || err.status === 409)
      ) {
        showToast("Só jobs concluídos/falhos/cancelados podem ser excluídos.", "info");
        setDeleteTarget(null);
        return;
      }
      if (err instanceof ApiError && err.code === "not_found") {
        showToast("Job já havia sido removido.", "info");
        setDeleteTarget(null);
        await fetchData();
        return;
      }
      if (err instanceof ApiError && err.code === "queue_unavailable") {
        showToast("Manager indisponível, tente de novo.", "error");
        return;
      }
      showToast("Falha ao excluir job.", "error");
    } finally {
      setDeleteBusy(false);
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
      if (typeof window !== "undefined" && job.datasetId) {
        window.dispatchEvent(
          new CustomEvent("hephaestus:dataset-updated", {
            detail: { datasetId: job.datasetId },
          }),
        );
      }
      setApplyOverwrite(false);
      await fetchData();
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

  // Ação de aplicar legendas do AutoLabel
  async function handleApplyCaptions(job: Job) {
    setApplyBusy(true);
    try {
      const result = await applyAutolabelCaptions(job.id, {
        datasetId: job.datasetId,
        overwrite: applyOverwrite,
      });
      showToast(
        `${result.applied} legendas aplicadas, ${result.skipped} ignoradas em ${result.images} imagens.`,
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
      await fetchData();
    } catch (err) {
      if (err instanceof ApiError) {
        showToast(autolabelErrorMessage(err.code), "error");
        return;
      }
      showToast("Falha ao aplicar legendas ao dataset.", "error");
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

  function handleResume(job: Job, art: JobArtifact) {
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

    onClose();
    router.push(
      `/difusao?checkpointId=${art.id}&checkpointName=${encodeURIComponent(checkpointName)}&epochOffset=${epochOffset}`
    );
  }

  function handleRerun(job: Job) {
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

      onClose();
      router.push(`/difusao?datasetId=${job.datasetId}`);
    } else {
      onClose();
      router.push(`/treino?datasetId=${job.datasetId}`);
    }
  }

  // Notificações do sistema agnósticas (telemetria, nó orquestrador, alertas de recursos)
  const systemNotifications = useMemo<SystemNotification[]>(() => {
    const list: SystemNotification[] = [];

    // 1. Alertas e telemetria de Hardware
    if (telemetry) {
      const vramUsedMb = telemetry.vramUsed ?? 0;
      const vramTotalMb = telemetry.vramTotal ?? 0;
      const vramPct =
        vramTotalMb > 0 ? Math.round((vramUsedMb / vramTotalMb) * 100) : 0;

      if (vramPct >= 85) {
        list.push({
          id: "sys-vram-alert",
          title: "Alerta de VRAM Elevada",
          message: `Uso de memória da GPU atingiu ${vramPct}% (${(vramUsedMb / 1024).toFixed(1)} GB de ${(vramTotalMb / 1024).toFixed(1)} GB).`,
          category: "infra",
          level: "warning",
          timestamp: new Date().toISOString(),
          actionLabel: "Ver no Painel",
          actionHref: "/dashboard",
        });
      }

      if (telemetry.cpu !== null && telemetry.cpu >= 90) {
        list.push({
          id: "sys-cpu-load",
          title: "Carga Intensa de CPU",
          message: `Processador do nó em ${telemetry.cpu}% de carga com ${telemetry.jobsActive} execução(ões) ativa(s).`,
          category: "infra",
          level: "warning",
          timestamp: new Date().toISOString(),
          actionLabel: "Ver Métricas",
          actionHref: "/dashboard",
        });
      }

    }

    // 2. Alertas de Jobs que falharam
    const failedJobs = jobs.filter((j) => j.status === "failed");
    failedJobs.slice(0, 2).forEach((job) => {
      list.push({
        id: `sys-job-failed-${job.id}`,
        title: `Falha na Execução: ${job.model}`,
        message:
          job.queueReason ||
          `A tarefa de ${job.kind === "yolo_train" ? "treino" : "processamento"} foi interrompida no nó local.`,
        category: "model",
        level: "error",
        timestamp: job.finishedAt || job.createdAt,
        actionLabel: "Investigar",
        actionHref: `/jobs?job=${job.id}`,
      });
    });

    return list;
  }, [telemetry, jobs]);

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
          j.status === "queued" ||
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
          j.status === "queued" ||
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

  function getJobServiceInfo(job: Job) {
    if (job.kind === "yolo_train") {
      return {
        serviceTitle: "Treino YOLO",
        categoryLabel: "Visão Computacional",
        icon: IconTarget,
        actionText: "Ver na Forja →",
        targetHref: `/jobs?job=${job.id}`,
      };
    }
    if (job.kind === "autotracker") {
      return {
        serviceTitle: "AutoTracker",
        categoryLabel: "Rastreamento & Vídeo",
        icon: IconLayers,
        actionText: job.datasetId ? "Ver no Dataset →" : "Ver no Studio →",
        targetHref: job.datasetId ? `/datasets/${job.datasetId}` : `/jobs?job=${job.id}`,
      };
    }
    const kindName = String(job.kind).replace(/_/g, " ");
    return {
      serviceTitle: kindName.charAt(0).toUpperCase() + kindName.slice(1),
      categoryLabel: "Processamento IA",
      icon: IconZap,
      actionText: "Ver Detalhes →",
      targetHref: `/jobs?job=${job.id}`,
    };
  }

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
              <span className="mr-1 flex items-center space-x-1.5 rounded-full border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm px-2.5 py-0.5 font-mono text-[11px] font-medium text-brand-300">
                <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
                <span className="tabular-nums">
                  {totalActiveCount} ativo{totalActiveCount > 1 ? "s" : ""}
                </span>
              </span>
            )}
            <button
              type="button"
              onClick={() => setCleanupOpen(true)}
              title="Limpar jobs antigos"
              aria-label="Limpar jobs antigos"
              className="inline-flex size-10 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer"
            >
              <IconTrash className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={() => void fetchData()}
              title="Atualizar atividades e telemetria"
              aria-label="Atualizar atividades"
              disabled={loading}
              className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer disabled:opacity-50"
            >
              <IconRefresh
                className={`size-3.5 ${loading ? "animate-spin motion-reduce:animate-none text-brand-400" : ""}`}
              />
            </button>
          </div>
        }
        footer={
          <div className="p-3.5 flex flex-col sm:flex-row items-center justify-between gap-3">
            <div className="flex items-center space-x-2 text-[11px] font-mono text-zinc-400">
              <span className={`size-2 rounded-full ${telemetry ? "bg-[#34d399] animate-pulse motion-reduce:animate-none" : "bg-zinc-500"}`} />
              <span>
                Nó Local:{" "}
                <strong className="text-zinc-200 font-semibold">
                  {telemetry
                    ? `${telemetry.jobsActive} ativo(s)${telemetry.vramUsed !== null ? ` · ${(telemetry.vramUsed / 1024).toFixed(1)} GB VRAM` : ""}${telemetry.cpu !== null ? ` · ${telemetry.cpu}% CPU` : ""}`
                    : "Sem telemetria"}
                </strong>
              </span>
            </div>

            <div className="flex items-center space-x-1.5 w-full sm:w-auto justify-end">
              <button
                type="button"
                onClick={() => {
                  onClose();
                  router.push("/dashboard");
                }}
                className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-brand-500/30 hover:bg-white/[0.08] hover:text-white cursor-pointer"
                title="Abrir Painel Geral do Nó"
              >
                <IconServer className="size-3.5 text-brand-400" />
                <span>Painel</span>
              </button>

              <button
                type="button"
                onClick={() => {
                  onClose();
                  router.push("/datasets");
                }}
                className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1.5 text-xs font-medium text-zinc-300 transition hover:border-brand-500/30 hover:bg-white/[0.08] hover:text-white cursor-pointer"
                title="Abrir Datasets"
              >
                <IconDatabase className="size-3.5 text-brand-400" />
                <span>Datasets</span>
              </button>

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
                { id: "all", label: "Tudo", count: sortedJobs.length + systemNotifications.length },
                { id: "active", label: "Em andamento", count: totalActiveCount },
                { id: "jobs", label: "Tarefas & Treino", count: sortedJobs.length },
                { id: "system", label: "Sistema & Alertas", count: systemNotifications.length },
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
              placeholder="Pesquisar tarefas, modelos ou alertas do sistema…"
              aria-label="Pesquisar atividades e notificações"
            />
          </div>

          {/* Lista Unificada de Atividades, Notificações & Jobs */}
          <div className="flex-1 overflow-y-auto p-4 space-y-3 [scrollbar-width:thin]">
            {filteredNotifications.length === 0 && filteredJobs.length === 0 ? (
              /* Empty State Agnóstico com Ações Rápidas do Estúdio */
              <div className="glass-card flex flex-col items-center gap-3.5 rounded-2xl p-8 text-center mt-6 border border-white/10">
                <span className="flex size-12 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
                  <IconActivity className="size-6 text-brand-400" />
                </span>
                <div className="max-w-sm space-y-1">
                  <h3 className="font-display text-sm font-semibold text-zinc-200">
                    {query ? `Nenhum resultado para "${query}"` : "Nenhuma atividade recente"}
                  </h3>
                  <p className="text-xs text-zinc-400 leading-relaxed">
                    O Centro de Atividades reúne as notificações do sistema em tempo real — treinos e tarefas de processamento, falhas de execução e alertas de recursos do nó (VRAM e CPU).
                  </p>
                </div>

                {/* Central de Ações Rápidas do Estúdio */}
                <div className="mt-3 w-full grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-3 border-t border-white/5">
                  <button
                    type="button"
                    onClick={() => {
                      onClose();
                      router.push("/datasets");
                    }}
                    className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
                  >
                    <IconDatabase className="size-4 text-brand-400" />
                    <span className="text-xs font-medium text-zinc-200">Datasets</span>
                    <span className="text-[10px] text-zinc-400 font-mono">Gerenciar acervo</span>
                  </button>

                  <button
                    type="button"
                    onClick={() => {
                      onClose();
                      router.push("/jobs");
                    }}
                    className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
                  >
                    <IconTarget className="size-4 text-brand-400" />
                    <span className="text-xs font-medium text-zinc-200">Forja do YOLO</span>
                    <span className="text-[10px] text-zinc-400 font-mono">Treino de visão</span>
                  </button>

                  <button
                    type="button"
                    onClick={() => {
                      onClose();
                      router.push("/dashboard");
                    }}
                    className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
                  >
                    <IconServer className="size-4 text-brand-400" />
                    <span className="text-xs font-medium text-zinc-200">Painel do Nó</span>
                    <span className="text-[10px] text-zinc-400 font-mono">Monitorar nós</span>
                  </button>
                </div>
              </div>
            ) : (
              <>
                {/* 1. Bloco de Notificações do Sistema */}
                {filteredNotifications.length > 0 && (
                  <div className="space-y-2">
                    {tab === "all" && filteredJobs.length > 0 && (
                      <div className="flex items-center justify-between px-1">
                        <span className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-400">
                          Notificações do Sistema ({filteredNotifications.length})
                        </span>
                      </div>
                    )}
                    {filteredNotifications.map((notif) => {
                      const levelConfig = {
                        info: {
                          border: "border-blue-500/30",
                          bg: "bg-blue-500/10",
                          text: "text-blue-400",
                          icon: IconInfo,
                        },
                        warning: {
                          border: "border-amber-500/30",
                          bg: "bg-amber-500/10",
                          text: "text-amber-400",
                          icon: IconAlertTriangle,
                        },
                        success: {
                          border: "border-[#34d399]/30",
                          bg: "bg-[#34d399]/10",
                          text: "text-[#34d399]",
                          icon: IconCheck,
                        },
                        error: {
                          border: "border-rose-500/30",
                          bg: "bg-rose-500/10",
                          text: "text-rose-400",
                          icon: IconAlertTriangle,
                        },
                      }[notif.level];

                      const IconComp = levelConfig.icon;
                      const categoryName = {
                        infra: "Infraestrutura",
                        orchestrator: "Orquestrador",
                        dataset: "Datasets",
                        model: "Modelos",
                      }[notif.category];

                      return (
                        <div
                          key={notif.id}
                          className="glass-card group relative overflow-hidden rounded-xl border border-white/10 p-3.5 transition-all duration-200 hover:border-brand-500/30 hover:bg-white/[0.03]"
                        >
                          <div className="flex items-start gap-3">
                            <span
                              className={`mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-lg border ${levelConfig.border} ${levelConfig.bg} ${levelConfig.text} backdrop-blur-sm`}
                            >
                              <IconComp className="size-3.5" />
                            </span>
                            <div className="min-w-0 flex-1">
                              <div className="flex items-baseline justify-between gap-2">
                                <h4 className="text-xs font-semibold text-zinc-100 truncate">
                                  {notif.title}
                                </h4>
                                <span className="font-mono text-[10px] text-zinc-400 shrink-0">
                                  {formatRelativeTime(notif.timestamp)}
                                </span>
                              </div>
                              <p className="mt-1 text-xs text-zinc-300 leading-relaxed">
                                {notif.message}
                              </p>
                              <div className="mt-2 flex items-center justify-between gap-2 pt-2 border-t border-white/5">
                                <span className="rounded border border-white/10 bg-white/[0.03] px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-caps text-zinc-400">
                                  {categoryName}
                                </span>
                                {notif.actionLabel && notif.actionHref && (
                                  <button
                                    type="button"
                                    onClick={() => {
                                      onClose();
                                      router.push(notif.actionHref!);
                                    }}
                                    className="text-brand-400 hover:text-brand-300 font-mono text-[11px] underline underline-offset-2 cursor-pointer"
                                  >
                                    {notif.actionLabel} →
                                  </button>
                                )}
                              </div>
                            </div>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* 2. Bloco de Tarefas & Treinamentos (Jobs) */}
                {filteredJobs.length > 0 && (
                  <div className="space-y-2">
                    {tab === "all" && filteredNotifications.length > 0 && (
                      <div className="flex items-center justify-between px-1 pt-2">
                        <span className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-400">
                          Tarefas & Treinamentos ({filteredJobs.length})
                        </span>
                      </div>
                    )}
                    {filteredJobs.map((job) => {
                      const config = STATUS_CONFIG[job.status] || STATUS_CONFIG.queued;
                      const serviceInfo = getJobServiceInfo(job);
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
                      const latestMetric = latestTrainingMetric(jobExtraMetrics);
                      const caps = jobCapabilities(job);

                      return (
                        <div
                          key={job.id}
                          className={`glass-card group relative overflow-hidden rounded-xl border transition-all duration-200 ${
                            isExpanded
                              ? "border-brand-500/40 bg-brand-500/[0.08] shadow-lg shadow-brand-500/5 ring-1 ring-brand-500/20"
                              : "border-white/10 hover:border-brand-500/30 hover:bg-white/[0.04]"
                          }`}
                        >
                          {/* Linha vertical de status */}
                          <div
                            className={`absolute top-0 bottom-0 left-0 w-1 ${config.borderClass}`}
                            aria-hidden="true"
                          />

                          <div
                            className="p-3 pl-4 cursor-pointer select-none"
                            onClick={() => toggleExpand(job.id)}
                            role="button"
                            tabIndex={0}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" || e.key === " ") {
                                e.preventDefault();
                                toggleExpand(job.id);
                              }
                            }}
                            aria-expanded={isExpanded}
                            aria-label={`${job.model} - ${config.label}`}
                          >
                            {/* Top row */}
                            <div className="flex items-start justify-between gap-2.5">
                              <div className="flex items-start space-x-2.5 min-w-0 flex-1">
                                {/* Ícone de status */}
                                <div
                                  className={`mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full ${config.iconBg} ${config.iconColor}`}
                                >
                                  {isRunning ? (
                                    <IconRefresh className="size-3.5 animate-spin motion-reduce:animate-none" />
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
                                    <span className="font-mono text-[11px] text-brand-400 shrink-0">
                                      · {serviceInfo.serviceTitle}
                                    </span>
                                    <span className="font-mono text-[11px] text-zinc-500 shrink-0">
                                      · {formatRelativeTime(job.createdAt)}
                                    </span>
                                  </div>

                                  <p className="mt-0.5 font-mono text-[11px] text-zinc-400 truncate">
                                    {job.engine} · Duração: {duration}
                                    {job.queuePosition !== null &&
                                      job.queuePosition !== undefined &&
                                      ` · Fila: #${job.queuePosition}`}
                                  </p>
                                </div>
                              </div>

                              {/* Status Badge + Chevron */}
                              <div className="flex items-center space-x-1.5 shrink-0">
                                <Badge variant={jobStatusToBadgeVariant(job.status)}>
                                  {config.label}
                                </Badge>
                                <span
                                  className={`text-zinc-500 transition-transform duration-200 ${
                                    isExpanded ? "rotate-180" : ""
                                  }`}
                                  aria-hidden="true"
                                >
                                  <IconChevronDown className="size-3.5" />
                                </span>
                              </div>
                            </div>

                            {/* Barra de Progresso em jobs ativos */}
                            {isActive && (
                              <ProgressBar
                                value={pct}
                                variant="brand"
                                size="md"
                                label={
                                  job.kind === "autolabel" || job.engine === "autolabel"
                                    ? `Legendagem Automática VLM · ${pct}%`
                                    : job.kind === "autotracker" || job.engine === "autotracker"
                                    ? `Rastreamento & Detecção · ${pct}%`
                                    : (job.kind as string) === "diffusion" || job.engine === "diffusion"
                                    ? `Treinamento LoRA Difusão · ${pct}%`
                                    : job.kind === "yolo_predict" || job.mode === "predict"
                                    ? `Inferência YOLO · ${pct}%`
                                    : `Treinamento YOLO · ${pct}%${latestMetric ? ` (Época ${latestMetric.epoch})` : ""}`
                                }
                                showPercent
                                className="mt-2.5"
                              />
                            )}

                            {/* Painel expansível: Detalhes, Métricas, Logs, Ações */}
                            {isExpanded && (
                              <div
                                className="mt-3 border-t border-white/10 pt-3 text-[11px] font-mono space-y-3 bg-black/40 backdrop-blur-md -mx-3 -mb-3 p-3.5"
                                onClick={(e) => e.stopPropagation()}
                              >
                                {/* Info chips com Nó Executor */}
                                <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 text-[11px]">
                                  <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                    <span className="text-zinc-400 block uppercase tracking-caps text-[10px] font-mono">
                                      Job ID
                                    </span>
                                    <span className="text-zinc-200 font-mono truncate block text-[11px]" title={job.id}>
                                      {job.id}
                                    </span>
                                  </div>

                                  {job.datasetId ? (
                                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 flex items-center justify-between">
                                      <div className="min-w-0 flex-1 mr-1">
                                        <span className="text-zinc-400 block uppercase tracking-caps text-[10px] font-mono">
                                          Dataset
                                        </span>
                                        <span className="text-zinc-200 font-mono truncate block text-[11px]" title={job.datasetId}>
                                          {job.datasetId.slice(0, 8)}…
                                        </span>
                                      </div>
                                      <button
                                        type="button"
                                        onClick={() => {
                                          onClose();
                                          router.push(`/datasets/${job.datasetId}`);
                                        }}
                                        className="text-brand-400 hover:text-brand-300 text-[11px] font-mono underline underline-offset-2 shrink-0 cursor-pointer"
                                      >
                                        Abrir
                                      </button>
                                    </div>
                                  ) : (
                                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                      <span className="text-zinc-400 block uppercase tracking-caps text-[10px] font-mono">
                                        Categoria
                                      </span>
                                      <span className="text-zinc-300 font-mono text-[11px]">{serviceInfo.categoryLabel}</span>
                                    </div>
                                  )}

                                  <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 col-span-2 sm:col-span-1">
                                    <span className="text-zinc-400 block uppercase tracking-caps text-[10px] font-mono">
                                      Nó Executor
                                    </span>
                                    <div className="flex items-center gap-1.5 mt-0.5 min-w-0">
                                      <span className="text-zinc-200 font-mono truncate block text-[11px]" title={job.orchestratorName || "Local"}>
                                        {job.orchestratorName || "Orquestrador Local"}
                                      </span>
                                      {job.orchestratorKind && (
                                        <span className="rounded bg-white/10 px-1 py-0.2 text-[9px] font-mono text-zinc-300 uppercase shrink-0">
                                          {job.orchestratorKind}
                                        </span>
                                      )}
                                      {job.orchestratorFallback && (
                                        <span className="rounded bg-amber-500/20 px-1 py-0.2 text-[9px] font-mono text-amber-300 shrink-0" title="Fallback automático ativado">
                                          fb
                                        </span>
                                      )}
                                    </div>
                                  </div>
                                </div>

                                {/* Métricas ao vivo/finais — regido por caps.metricChips */}
                                {caps.metricChips === "progress" && (
                                  <div className="grid grid-cols-1 gap-1.5 text-center">
                                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                      <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Imagens processadas</span>
                                      <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                                        {imageProgressLabel(job.step, job.progress)}
                                      </span>
                                    </div>
                                  </div>
                                )}
                                {caps.metricChips !== null && caps.metricChips !== "progress" && latestMetric && (
                                  <div>
                                    <div className="text-[10px] font-mono text-zinc-400 uppercase tracking-caps mb-1.5 flex items-center justify-between">
                                      <span>
                                        {caps.metricChips === "diffusion"
                                          ? "Métricas Difusão LoRA"
                                          : "Métricas"}
                                      </span>
                                      <span className="text-zinc-400">Epoch {latestMetric.epoch}</span>
                                    </div>
                                    {caps.metricChips === "diffusion" ? (
                                      <div className="grid grid-cols-4 gap-1.5 text-center">
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Loss</span>
                                          <span className="text-xs font-semibold text-indigo-400 font-mono tabular-nums">
                                            {latestMetric.loss != null ? latestMetric.loss.toFixed(4) : "—"}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">LR</span>
                                          <span className="text-xs font-semibold text-sky-400 font-mono tabular-nums">
                                            {latestMetric.lr ? latestMetric.lr.toExponential(1) : "—"}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Step</span>
                                          <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                                            {latestMetric.step ?? latestMetric.epoch * 10}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Época</span>
                                          <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                                            {latestMetric.epoch}
                                          </span>
                                        </div>
                                      </div>
                                    ) : (
                                      <div className="grid grid-cols-4 gap-1.5 text-center">
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">mAP50</span>
                                          <span className="text-xs font-semibold text-[#34d399] font-mono tabular-nums">
                                            {latestMetric.map50 !== undefined ? `${(latestMetric.map50 * 100).toFixed(1)}%` : "—"}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">mAP50-95</span>
                                          <span className="text-xs font-semibold text-[#34d399] font-mono tabular-nums">
                                            {latestMetric.map5095 !== undefined ? `${(latestMetric.map5095 * 100).toFixed(1)}%` : "—"}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Box Loss</span>
                                          <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                                            {latestMetric.boxLoss?.toFixed(3) ?? "—"}
                                          </span>
                                        </div>
                                        <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                                          <span className="text-[10px] font-mono text-zinc-400 block uppercase tracking-caps">Cls Loss</span>
                                          <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                                            {latestMetric.clsLoss?.toFixed(3) ?? "—"}
                                          </span>
                                        </div>
                                      </div>
                                    )}
                                  </div>
                                )}

                                {/* Galeria de amostras visuais de validação — gated por caps */}
                                {caps.samplesGallery && jobExtraArtifacts && jobExtraArtifacts.length > 0 && (
                                  <JobSamplesGallery
                                    jobId={job.id}
                                    artifacts={jobExtraArtifacts}
                                    onDownload={(jId, art) => handleDownload(jId, art)}
                                  />
                                )}

                                {/* Outros artefatos disponíveis para download */}
                                {jobExtraArtifacts && jobExtraArtifacts.length > 0 && (
                                  <JobArtifactsList
                                    jobId={job.id}
                                    artifacts={jobExtraArtifacts.filter(
                                      (art) =>
                                        art.kind !== "sample" &&
                                        !art.path.startsWith("samples/") &&
                                        !art.path.includes("sample_epoch_"),
                                    )}
                                    onDownload={(jId, art) => handleDownload(jId, art)}
                                    onResume={(jId, art) => handleResume(job, art)}
                                  />
                                )}

                                {/* Mensagem de Erro com cópia rápida e destaque */}
                                {(job.error || job.queueReason) && job.status === "failed" && (
                                  <div className="rounded-lg bg-rose-950/40 border border-rose-800/50 p-2.5 text-rose-300 text-[11px] font-mono space-y-1">
                                    <div className="flex items-center justify-between font-semibold text-rose-200">
                                      <span className="flex items-center gap-1.5">
                                        <span className="size-1.5 rounded-full bg-rose-500 animate-pulse" />
                                        Falha no Orquestrador
                                      </span>
                                      <button
                                        type="button"
                                        onClick={() => {
                                          void copyToClipboard(job.error || job.queueReason || "");
                                          showToast("Traceback copiado para a área de transferência", "info");
                                        }}
                                        className="text-[10px] text-rose-400 hover:text-rose-200 underline cursor-pointer"
                                      >
                                        Copiar Erro
                                      </button>
                                    </div>
                                    <p className="whitespace-pre-wrap break-all text-[10px] text-rose-300/90 font-mono max-h-32 overflow-y-auto leading-relaxed select-text">
                                      {job.error || job.queueReason}
                                    </p>
                                  </div>
                                )}

                                {/* Terminal de Logs e Telemetria Integrado */}
                                <div className="pt-1">
                                  <JobLogViewer
                                    job={job}
                                    metrics={metrics[job.id] || (job.metrics ?? [])}
                                    artifacts={artifacts[job.id] || []}
                                    compact
                                  />
                                </div>

                                {/* Ações contextuais */}
                                <div className="pt-2.5 border-t border-white/10 flex items-center justify-between gap-2 flex-wrap">
                                  {/* AutoTracker: aplicar boxes — gated por caps.applyAction */}
                                  {caps.applyAction && job.kind === "autotracker" && job.status === "done" && (
                                    <div className="flex items-center gap-2 flex-wrap">
                                      <button
                                        type="button"
                                        onClick={() => setAutotrackerReviewJob(job)}
                                        className="inline-flex items-center gap-1 rounded-lg border border-brand-500/40 bg-brand-500/15 px-2.5 py-1 text-[11px] font-medium text-brand-300 transition hover:bg-brand-500/25 active:scale-[0.985] cursor-pointer"
                                        title="Revisar classes detectadas e aceitar novas classes antes de aplicar"
                                      >
                                        <IconSparkles className="size-3 text-brand-400" />
                                        <span>Revisar e Aplicar</span>
                                      </button>
                                      <label className="flex items-center gap-1.5 text-[11px] text-zinc-400 cursor-pointer">
                                        <input
                                          type="checkbox"
                                          checked={applyOverwrite}
                                          onChange={(e) => setApplyOverwrite(e.target.checked)}
                                          className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
                                        />
                                        <span>Sobrescrever</span>
                                      </label>
                                      <button
                                        type="button"
                                        disabled={applyBusy}
                                        onClick={() => handleApplyBoxes(job)}
                                        className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.06] px-2.5 py-1 text-[11px] font-medium text-zinc-300 transition hover:bg-white/10 active:scale-[0.985] disabled:opacity-50 cursor-pointer"
                                      >
                                        <IconCheck className="size-3" />
                                        <span>{applyBusy ? "Aplicando…" : "Aplicação direta"}</span>
                                      </button>
                                    </div>
                                  )}

                                  {/* AutoLabel: aplicar legendas — gated por caps.applyAction */}
                                  {caps.applyAction && job.kind === "autolabel" && job.status === "done" && (
                                    <div className="flex items-center gap-2 flex-wrap">
                                      <button
                                        type="button"
                                        onClick={() => setReviewJob(job)}
                                        className="inline-flex items-center gap-1 rounded-lg border border-brand-500/40 bg-brand-500/15 px-2.5 py-1 text-[11px] font-medium text-brand-300 transition hover:bg-brand-500/25 active:scale-[0.985] cursor-pointer"
                                        title="Inspecionar, editar e curar legendas antes de aplicar"
                                      >
                                        <IconSparkles className="size-3 text-brand-400" />
                                        <span>Revisar Legendas</span>
                                      </button>
                                      <label className="flex items-center gap-1.5 text-[11px] text-zinc-400 cursor-pointer">
                                        <input
                                          type="checkbox"
                                          checked={applyOverwrite}
                                          onChange={(e) => setApplyOverwrite(e.target.checked)}
                                          className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
                                        />
                                        <span>Sobrescrever</span>
                                      </label>
                                      <button
                                        type="button"
                                        disabled={applyBusy}
                                        onClick={() => handleApplyCaptions(job)}
                                        className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1 text-[11px] font-medium text-zinc-300 transition hover:bg-white/10 active:scale-[0.985] disabled:opacity-50 cursor-pointer"
                                        title="Aplicar todas as legendas direto sem inspeção"
                                      >
                                        <IconCheck className="size-3 text-zinc-400" />
                                        <span>{applyBusy ? "Aplicando…" : "Aplicar Todas"}</span>
                                      </button>
                                    </div>
                                  )}

                                  {/* Repetir treino para jobs finalizados/falhados — gated por caps.rerun */}
                                  {caps.rerun && !isActive && (job.engine === "diffusion" || job.engine === "yolo") && (
                                    <button
                                      type="button"
                                      onClick={() => handleRerun(job)}
                                      className="inline-flex items-center gap-1 rounded-lg border border-brand-500/40 bg-brand-500/15 px-2.5 py-1 text-[11px] font-medium text-brand-300 transition hover:bg-brand-500/25 active:scale-[0.985] cursor-pointer"
                                      title="Abrir a Forja pré-carregada com todos os parâmetros deste treino para submeter novamente"
                                    >
                                      <IconRefresh className="size-3 text-brand-400" />
                                      <span>Repetir Treino</span>
                                    </button>
                                  )}

                                  {/* Cancelar Job ativo */}
                                  {isActive && (
                                    <button
                                      type="button"
                                      onClick={() => setAbortTarget(job)}
                                      className="inline-flex items-center gap-1 rounded-lg border border-rose-500/40 bg-rose-500/15 px-2.5 py-1 text-[11px] font-medium text-rose-300 transition hover:bg-rose-500/25 active:scale-[0.985] cursor-pointer"
                                    >
                                      <IconTrash className="size-3" />
                                      <span>Cancelar Job</span>
                                    </button>
                                  )}

                                  {/* Acompanhar (tela cheia / modo foco) */}
                                  <button
                                    type="button"
                                    onClick={() => {
                                      onClose();
                                      router.push(`/jobs?job=${job.id}&focus=1`);
                                    }}
                                    className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 min-h-[40px] text-[11px] font-mono text-zinc-300 transition hover:bg-white/[0.06] active:scale-[0.985] cursor-pointer"
                                    title="Acompanhar este job em tela cheia"
                                  >
                                    <IconActivity className="size-3" />
                                    <span>Acompanhar</span>
                                  </button>

                                  {/* Excluir job terminal */}
                                  {!isActive && (
                                    <button
                                      type="button"
                                      onClick={() => setDeleteTarget(job)}
                                      className="inline-flex items-center gap-1 rounded-lg border border-white/10 bg-white/[0.03] px-2.5 py-1 min-h-[40px] text-[11px] font-mono text-rose-300 transition hover:border-rose-500/40 hover:bg-rose-500/10 active:scale-[0.985] cursor-pointer"
                                      aria-label="Excluir job"
                                      title="Excluir este job e seus artefatos"
                                    >
                                      <IconTrash className="size-3" />
                                      <span>Excluir</span>
                                    </button>
                                  )}

                                  {/* Ver detalhes no studio */}
                                  <button
                                    type="button"
                                    onClick={() => {
                                      onClose();
                                      router.push(serviceInfo.targetHref);
                                    }}
                                    className="ml-auto text-zinc-400 hover:text-zinc-200 text-[11px] font-mono underline underline-offset-2 cursor-pointer"
                                  >
                                    {serviceInfo.actionText}
                                  </button>
                                </div>
                              </div>
                            )}
                          </div>
                        </div>
                      );
                    })}
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
        terminalJobs={jobs
          .filter((j) => j.status === "done" || j.status === "failed" || j.status === "cancelled")
          .map((j) => ({ id: j.id, status: j.status as "done" | "failed" | "cancelled", createdAt: j.createdAt, finishedAt: j.finishedAt }))}
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
