"use client";

import { useRouter } from "next/navigation";
import {
  IconActivity,
  IconCheck,
  IconChevronDown,
  IconRefresh,
  IconSparkles,
  IconTarget,
  IconTrash,
  IconX,
} from "@/components/icons";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import {
  Badge,
  Button,
  jobStatusToBadgeVariant,
  ProgressBar,
} from "@/components/ui";
import { copyToClipboard } from "@/lib/clipboard";
import { formatDuration, formatRelativeTime } from "@/lib/format";
import { imageProgressLabel, jobCapabilities } from "@/lib/jobCapabilities";
import { latestTrainingMetric } from "@/lib/jobMetrics";
import type {
  Job,
  JobArtifact,
  JobMetrics as JobMetricsType,
} from "@/types/studio";
import { friendlyJobError } from "@/types/studio";
import { JOB_STATUS_CONFIG, JobArtifactsList } from "../JobCard";
import { JobSamplesGallery } from "../JobSamplesGallery";
import { showToast } from "@/components/ui/Toast";
import { getJobServiceInfo } from "./jobServiceInfo";

export interface ActionCenterJobItemProps {
  job: Job;
  isExpanded: boolean;
  onToggleExpand: (id: string) => void;
  metrics?: JobMetricsType[];
  artifacts?: JobArtifact[];
  onClose: () => void;
  onDownload: (jobId: string, art: JobArtifact) => void;
  onResume: (job: Job, art: JobArtifact) => void;
  onRerun: (job: Job) => void;
  onAbort: (job: Job) => void;
  onDelete: (job: Job) => void;
  onReviewAutolabel: (job: Job) => void;
  onReviewAutotracker: (job: Job) => void;
  applyBusy: boolean;
  applyOverwrite: boolean;
  setApplyOverwrite: (overwrite: boolean) => void;
  onApplyBoxes: (job: Job) => void;
  onApplyCaptions: (job: Job) => void;
}

const STATUS_CONFIG = JOB_STATUS_CONFIG;

export function ActionCenterJobItem({
  job,
  isExpanded,
  onToggleExpand,
  metrics,
  artifacts,
  onClose,
  onDownload,
  onResume,
  onRerun,
  onAbort,
  onDelete,
  onReviewAutolabel,
  onReviewAutotracker,
  applyBusy,
  applyOverwrite,
  setApplyOverwrite,
  onApplyBoxes,
  onApplyCaptions,
}: ActionCenterJobItemProps) {
  const router = useRouter();

  const config = STATUS_CONFIG[job.status] || STATUS_CONFIG.queued;
  const serviceInfo = getJobServiceInfo(job);
  const isActive =
    job.status === "running" ||
    job.status === "preparing" ||
    job.status === "queued" ||
    job.status === "dispatched" ||
    job.status === "cancelling";
  const isRunning = job.status === "running";
  const pct = Math.round((job.progress ?? 0) * 100);
  const duration = formatDuration(job.createdAt, job.finishedAt);
  const latestMetric = latestTrainingMetric(metrics);
  const caps = jobCapabilities(job);

  return (
    <div
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

      <button
        type="button"
        onClick={() => onToggleExpand(job.id)}
        aria-expanded={isExpanded}
        aria-label={`${job.model} - ${config.label}`}
        className="block w-full p-3 pl-4 text-left cursor-pointer select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand-500/70"
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
                <span className="font-mono text-2xs text-brand-400 shrink-0">
                  · {serviceInfo.serviceTitle}
                </span>
                <span className="font-mono text-2xs text-zinc-500 shrink-0">
                  · {formatRelativeTime(job.createdAt)}
                </span>
              </div>

              <p className="mt-0.5 font-mono text-2xs text-zinc-400 truncate">
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
          <div className="mt-2.5 space-y-1.5">
            <ProgressBar
              value={pct}
              variant="brand"
              size="md"
              label={
                job.phaseMessage
                  ? job.phaseMessage
                  : job.kind === "autolabel" || job.engine === "autolabel"
                    ? `Legendagem Automática VLM · ${pct}%`
                    : job.kind === "autotracker" ||
                        job.engine === "autotracker"
                      ? `Rastreamento & Detecção · ${pct}%`
                      : (job.kind as string) === "diffusion_generate" ||
                          (job.kind === "diffusion" && job.mode === "generate")
                        ? `Geração de Imagens · ${pct}%`
                        : (job.kind as string) === "diffusion" ||
                            job.engine === "diffusion"
                          ? `Treinamento LoRA Difusão · ${pct}%`
                          : job.kind === "yolo_predict" ||
                              job.mode === "predict"
                            ? `Inferência YOLO · ${pct}%`
                            : `Treinamento YOLO · ${pct}%${latestMetric ? ` (Época ${latestMetric.epoch})` : ""}`
              }
              showPercent
            />
            {job.phaseMessage && (
              <div className="flex items-center justify-between text-3xs text-zinc-400 font-mono">
                <span className="truncate">{job.phaseMessage}</span>
                {job.vramUsedGb && (
                  <span className="shrink-0 text-zinc-500">
                    {job.vramUsedGb.toFixed(1)} GB VRAM
                  </span>
                )}
              </div>
            )}
          </div>
        )}
      </button>

      {/* Painel expansível: Detalhes, Métricas, Logs, Ações */}
      {isExpanded && (
        <div className="mt-3 border-t border-white/10 pt-3 text-2xs font-mono space-y-3 bg-black/55 p-3.5">
          {/* Info chips com Nó Executor */}
          <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 text-2xs">
            <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
              <span className="text-zinc-400 block uppercase tracking-caps text-3xs font-mono">
                Job ID
              </span>
              <span
                className="text-zinc-200 font-mono truncate block text-2xs"
                title={job.id}
              >
                {job.id}
              </span>
            </div>

            {job.datasetId ? (
              <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 flex items-center justify-between">
                <div className="min-w-0 flex-1 mr-1">
                  <span className="text-zinc-400 block uppercase tracking-caps text-3xs font-mono">
                    Dataset
                  </span>
                  <span
                    className="text-zinc-200 font-mono truncate block text-2xs"
                    title={job.datasetId}
                  >
                    {job.datasetId.slice(0, 8)}…
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => {
                    onClose();
                    router.push(`/datasets/${job.datasetId}`);
                  }}
                  className="text-brand-400 hover:text-brand-300 text-2xs font-mono underline underline-offset-2 shrink-0 cursor-pointer"
                >
                  Abrir
                </button>
              </div>
            ) : (
              <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                <span className="text-zinc-400 block uppercase tracking-caps text-3xs font-mono">
                  Categoria
                </span>
                <span className="text-zinc-300 font-mono text-2xs">
                  {serviceInfo.categoryLabel}
                </span>
              </div>
            )}

            <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10 col-span-2 sm:col-span-1">
              <span className="text-zinc-400 block uppercase tracking-caps text-3xs font-mono">
                Nó Executor
              </span>
              <div className="flex items-center gap-1.5 mt-0.5 min-w-0">
                <span
                  className="text-zinc-200 font-mono truncate block text-2xs"
                  title={job.orchestratorName || "Local"}
                >
                  {job.orchestratorName || "Orquestrador Local"}
                </span>
                {job.orchestratorKind && (
                  <span className="rounded bg-white/10 px-1 py-0.2 text-4xs font-mono text-zinc-300 uppercase shrink-0">
                    {job.orchestratorKind}
                  </span>
                )}
                {job.orchestratorFallback && (
                  <span
                    className="rounded bg-status-alert/20 px-1 py-0.2 text-4xs font-mono text-amber-300 shrink-0"
                    title="Fallback automático ativado"
                  >
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
                <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                  Imagens processadas
                </span>
                <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                  {imageProgressLabel(job.step, job.progress)}
                </span>
              </div>
            </div>
          )}
          {caps.metricChips !== null &&
            caps.metricChips !== "progress" &&
            latestMetric && (
              <div>
                <div className="text-3xs font-mono text-zinc-400 uppercase tracking-caps mb-1.5 flex items-center justify-between">
                  <span>
                    {caps.metricChips === "diffusion"
                      ? "Métricas Difusão LoRA"
                      : "Métricas"}
                  </span>
                  <span className="text-zinc-400">
                    Epoch {latestMetric.epoch}
                  </span>
                </div>
                {caps.metricChips === "diffusion" ? (
                  <div className="grid grid-cols-4 gap-1.5 text-center">
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        Loss
                      </span>
                      <span className="text-xs font-semibold text-indigo-400 font-mono tabular-nums">
                        {latestMetric.loss != null
                          ? latestMetric.loss.toFixed(4)
                          : "—"}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        LR
                      </span>
                      <span className="text-xs font-semibold text-sky-400 font-mono tabular-nums">
                        {latestMetric.lr
                          ? latestMetric.lr.toExponential(1)
                          : "—"}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        Step
                      </span>
                      <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                        {latestMetric.step ?? latestMetric.epoch * 10}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        Época
                      </span>
                      <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                        {latestMetric.epoch}
                      </span>
                    </div>
                  </div>
                ) : (
                  <div className="grid grid-cols-4 gap-1.5 text-center">
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        mAP50
                      </span>
                      <span className="text-xs font-semibold text-status-success font-mono tabular-nums">
                        {latestMetric.map50 !== undefined
                          ? `${(latestMetric.map50 * 100).toFixed(1)}%`
                          : "—"}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        mAP50-95
                      </span>
                      <span className="text-xs font-semibold text-status-success font-mono tabular-nums">
                        {latestMetric.map5095 !== undefined
                          ? `${(latestMetric.map5095 * 100).toFixed(1)}%`
                          : "—"}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        Box Loss
                      </span>
                      <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                        {latestMetric.boxLoss?.toFixed(3) ?? "—"}
                      </span>
                    </div>
                    <div className="rounded-lg bg-white/[0.03] backdrop-blur-sm p-2 border border-white/10">
                      <span className="text-3xs font-mono text-zinc-400 block uppercase tracking-caps">
                        Cls Loss
                      </span>
                      <span className="text-xs font-semibold text-zinc-200 font-mono tabular-nums">
                        {latestMetric.clsLoss?.toFixed(3) ?? "—"}
                      </span>
                    </div>
                  </div>
                )}
              </div>
            )}

          {/* Galeria de amostras visuais de validação — gated por caps */}
          {caps.samplesGallery && artifacts && artifacts.length > 0 && (
            <JobSamplesGallery
              jobId={job.id}
              artifacts={artifacts}
              onDownload={(jId, art) => onDownload(jId, art)}
            />
          )}

          {/* Outros artefatos disponíveis para download */}
          {artifacts && artifacts.length > 0 && (
            <JobArtifactsList
              jobId={job.id}
              artifacts={artifacts.filter(
                (art) =>
                  art.kind !== "sample" &&
                  !art.path.startsWith("samples/") &&
                  !art.path.includes("sample_epoch_"),
              )}
              onDownload={(jId, art) => onDownload(jId, art)}
              onResume={(_jId, art) => onResume(job, art)}
            />
          )}

          {/* Mensagem de Erro com cópia rápida e destaque */}
          {(job.error || job.queueReason) && job.status === "failed" && (
            <div className="rounded-lg bg-rose-950/40 border border-rose-800/50 p-2.5 text-rose-300 text-2xs font-mono space-y-1">
              <div className="flex items-center justify-between font-semibold text-rose-200">
                <span className="flex items-center gap-1.5">
                  <span className="size-1.5 rounded-full bg-rose-500 animate-pulse" />
                  Falha no Orquestrador
                </span>
                <button
                  type="button"
                  onClick={() => {
                    void copyToClipboard(job.error || job.queueReason || "");
                    showToast(
                      "Traceback copiado para a área de transferência",
                      "info",
                    );
                  }}
                  className="text-3xs text-rose-400 hover:text-rose-200 underline cursor-pointer"
                >
                  Copiar Erro
                </button>
              </div>
              <p className="whitespace-pre-wrap break-words text-3xs text-rose-300/90 font-mono max-h-32 overflow-y-auto leading-relaxed select-text">
                {friendlyJobError(job.error) || job.queueReason}
              </p>
            </div>
          )}

          {/* Terminal de Logs e Telemetria Integrado */}
          <div className="pt-1">
            <JobLogViewer
              job={job}
              metrics={metrics || []}
              artifacts={artifacts || []}
              compact
              livePhase={job.phase}
              livePhaseMessage={job.phaseMessage}
            />
          </div>

          {/* Ações contextuais */}
          <div className="pt-2.5 border-t border-white/10 flex items-center justify-between gap-2 flex-wrap">
            {/* AutoTracker: aplicar boxes — gated por caps.applyAction */}
            {caps.applyAction &&
              job.kind === "autotracker" &&
              job.status === "done" && (
                <div className="flex items-center gap-2 flex-wrap">
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => onReviewAutotracker(job)}
                    leftIcon={<IconSparkles />}
                    title="Revisar classes detectadas e aceitar novas classes antes de aplicar"
                  >
                    <span>Revisar e Aplicar</span>
                  </Button>
                  <label className="flex items-center gap-1.5 text-2xs text-zinc-400 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={applyOverwrite}
                      onChange={(e) => setApplyOverwrite(e.target.checked)}
                      className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
                    />
                    <span>Sobrescrever</span>
                  </label>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={applyBusy}
                    onClick={() => onApplyBoxes(job)}
                    leftIcon={<IconCheck />}
                  >
                    <span>
                      {applyBusy ? "Aplicando…" : "Aplicação direta"}
                    </span>
                  </Button>
                </div>
              )}

            {/* AutoLabel: aplicar legendas — gated por caps.applyAction */}
            {caps.applyAction &&
              job.kind === "autolabel" &&
              job.status === "done" && (
                <div className="flex items-center gap-2 flex-wrap">
                  <Button
                    variant="primary"
                    size="sm"
                    onClick={() => onReviewAutolabel(job)}
                    leftIcon={<IconSparkles />}
                    title="Inspecionar, editar e curar legendas antes de aplicar"
                  >
                    <span>Revisar Legendas</span>
                  </Button>
                  <label className="flex items-center gap-1.5 text-2xs text-zinc-400 cursor-pointer">
                    <input
                      type="checkbox"
                      checked={applyOverwrite}
                      onChange={(e) => setApplyOverwrite(e.target.checked)}
                      className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
                    />
                    <span>Sobrescrever</span>
                  </label>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={applyBusy}
                    onClick={() => onApplyCaptions(job)}
                    leftIcon={<IconCheck />}
                    title="Aplicar todas as legendas direto sem inspeção"
                  >
                    <span>{applyBusy ? "Aplicando…" : "Aplicar Todas"}</span>
                  </Button>
                </div>
              )}

            {/* Repetir treino para jobs finalizados/falhados — gated por caps.rerun */}
            {caps.rerun &&
              !isActive &&
              (job.engine === "diffusion" || job.engine === "yolo") && (
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => onRerun(job)}
                  leftIcon={<IconRefresh />}
                  title="Abrir a Forja pré-carregada com todos os parâmetros deste treino para submeter novamente"
                >
                  <span>Repetir Treino</span>
                </Button>
              )}

            {/* Cancelar Job ativo */}
            {isActive && (
              <Button
                variant="destructive"
                size="sm"
                onClick={() => onAbort(job)}
                leftIcon={<IconTrash />}
              >
                <span>Cancelar Job</span>
              </Button>
            )}

            {/* Acompanhar (tela cheia / modo foco) */}
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                onClose();
                router.push(`/jobs?job=${job.id}&focus=1`);
              }}
              leftIcon={<IconActivity />}
              title="Acompanhar este job em tela cheia"
            >
              <span>Acompanhar</span>
            </Button>

            {/* Excluir job terminal */}
            {!isActive && (
              <Button
                variant="destructive"
                size="sm"
                onClick={() => onDelete(job)}
                leftIcon={<IconTrash />}
                aria-label="Excluir job"
                title="Excluir este job e seus artefatos"
              >
                <span>Excluir</span>
              </Button>
            )}

            {/* Ver detalhes no studio */}
            <button
              type="button"
              onClick={() => {
                onClose();
                router.push(serviceInfo.targetHref);
              }}
              className="ml-auto text-zinc-400 hover:text-zinc-200 text-2xs font-mono underline underline-offset-2 cursor-pointer"
            >
              {serviceInfo.actionText}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
