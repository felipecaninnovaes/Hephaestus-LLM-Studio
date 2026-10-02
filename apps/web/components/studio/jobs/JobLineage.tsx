"use client";

import { useEffect, useState } from "react";
import {
  IconAlertTriangle,
  IconChevronDown,
  IconDatabase,
  IconNetwork,
  IconSparkles,
} from "@/components/icons";
import { Badge, jobStatusToBadgeVariant } from "@/components/ui/Badge";
import { Spinner } from "@/components/ui/Spinner";
import {
  type CheckpointInfo,
  type JobGroup,
  buildLineageLayout,
} from "@/lib/jobLineage";
import { getJobLineage } from "@/lib/jobs";
import type { JobStatus, LineageGraph, LineageNode } from "@/types/studio";

interface JobLineageProps {
  jobId: string;
  /** Jobs cujos artefatos já estão carregados na tela (habilita scroll-to-artifact). */
  loadedArtifactJobIds: Set<string>;
  onSelectJob: (jobId: string) => void;
  onSelectDataset: (datasetId: string) => void;
  onFocusArtifact: (jobId: string, artifactId: string) => void;
}

const STATUS_LABEL: Record<JobStatus, string> = {
  preparing: "Preparando",
  queued: "Na fila",
  dispatched: "Despachando",
  running: "Executando",
  cancelling: "Cancelando",
  done: "Concluído",
  failed: "Falhou",
  cancelled: "Cancelado",
};

function idSuffix(id: string): string {
  const uuid = id.includes(":") ? id.split(":")[1] : id;
  return uuid.slice(0, 8);
}

function stripPrefix(id: string): string {
  return id.includes(":") ? id.split(":").slice(1).join(":") : id;
}

interface CheckpointItemProps {
  info: CheckpointInfo;
  ownerJobId: string;
  loadedArtifactJobIds: Set<string>;
  onSelectJob: (jobId: string) => void;
  onFocusArtifact: (jobId: string, artifactId: string) => void;
}

function CheckpointItem({
  info,
  ownerJobId,
  loadedArtifactJobIds,
  onSelectJob,
  onFocusArtifact,
}: CheckpointItemProps) {
  const { node, resumedByJobId, usedByJobId } = info;
  const rawId = stripPrefix(node.id);
  const canFocus = loadedArtifactJobIds.has(ownerJobId);

  const meta: string[] = [];
  if (node.epoch != null) meta.push(`época ${node.epoch}`);
  meta.push(`#${idSuffix(node.id)}`);

  return (
    <div className="rounded-lg border border-white/5 bg-white/[0.02] p-2 space-y-1.5 transition-colors hover:border-white/15">
      <div className="flex items-center justify-between gap-2">
        {canFocus ? (
          <button
            type="button"
            onClick={() => onFocusArtifact(ownerJobId, rawId)}
            className="truncate text-left text-xs font-medium text-zinc-200 hover:text-brand-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded"
            title="Rolar até o artefato"
          >
            {node.label}
          </button>
        ) : (
          <span className="truncate text-xs font-medium text-zinc-300">
            {node.label}
          </span>
        )}
        <span className="shrink-0 font-mono text-3xs text-zinc-400">
          {meta.join(" · ")}
        </span>
      </div>

      {resumedByJobId && (
        <div className="flex items-center gap-1.5 pt-0.5 border-t border-white/5">
          <span className="text-3xs text-brand-300 font-mono">↳ retomado por</span>
          <button
            type="button"
            onClick={() => onSelectJob(resumedByJobId)}
            className="font-mono text-3xs text-brand-400 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded px-0.5"
            title={`Abrir job ${resumedByJobId}`}
          >
            #{resumedByJobId.slice(0, 8)}
          </button>
        </div>
      )}

      {usedByJobId && (
        <div className="flex items-center gap-1.5 pt-0.5 border-t border-white/5">
          <span className="text-3xs text-amber-300 font-mono">↳ usado por</span>
          <button
            type="button"
            onClick={() => onSelectJob(usedByJobId)}
            className="font-mono text-3xs text-amber-400 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded px-0.5"
            title={`Abrir job/geração ${usedByJobId}`}
          >
            #{usedByJobId.slice(0, 8)}
          </button>
        </div>
      )}
    </div>
  );
}

interface JobGroupCardProps {
  group: JobGroup;
  currentJobId: string;
  loadedArtifactJobIds: Set<string>;
  onSelectJob: (jobId: string) => void;
  onFocusArtifact: (jobId: string, artifactId: string) => void;
}

function JobGroupCard({
  group,
  currentJobId,
  loadedArtifactJobIds,
  onSelectJob,
  onFocusArtifact,
}: JobGroupCardProps) {
  const { job, checkpoints, generations, resumedFrom, usedFrom } = group;
  const rawJobId = stripPrefix(job.id);
  const isCurrent = rawJobId === currentJobId;
  const [expanded, setExpanded] = useState(false);

  const displayedCheckpoints =
    checkpoints.length > 5 && !expanded ? checkpoints.slice(0, 5) : checkpoints;

  return (
    <div
      className={`rounded-xl border p-3 space-y-3 transition-colors ${
        isCurrent
          ? "border-brand-400/60 bg-brand-500/[0.07]"
          : "border-white/10 bg-white/[0.02]"
      }`}
    >
      {/* Cabeçalho do Job */}
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => onSelectJob(rawJobId)}
              className="truncate text-xs font-semibold text-zinc-100 hover:text-brand-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded"
              title={`Abrir job ${rawJobId}`}
            >
              {job.label}
            </button>
            {isCurrent && (
              <Badge variant="brand" className="shrink-0">
                atual
              </Badge>
            )}
          </div>
          <div className="flex items-center gap-2 flex-wrap font-mono text-3xs text-zinc-400">
            {job.status && (
              <Badge
                variant={jobStatusToBadgeVariant(job.status as JobStatus)}
                className="shrink-0"
              >
                {STATUS_LABEL[job.status as JobStatus] ?? job.status}
              </Badge>
            )}
            {job.createdAt && (
              <span>{new Date(job.createdAt).toLocaleString("pt-BR")}</span>
            )}
            <span>#{idSuffix(job.id)}</span>
          </div>
        </div>
      </div>

      {/* Relação de origem: retomado de um checkpoint de outro job */}
      {resumedFrom && (
        <div className="rounded-lg border border-brand-400/30 bg-brand-500/10 p-2 text-2xs space-y-0.5">
          <span className="font-mono text-3xs uppercase tracking-caps text-brand-300">
            Retomado de
          </span>
          <div className="flex items-center gap-1.5 flex-wrap text-zinc-200">
            <span className="font-medium truncate max-w-[200px]" title={resumedFrom.checkpointLabel}>
              {resumedFrom.checkpointLabel}
            </span>
            {resumedFrom.parentJobId && (
              <>
                <span className="text-zinc-500">·</span>
                <button
                  type="button"
                  onClick={() => onSelectJob(resumedFrom.parentJobId!)}
                  className="font-mono text-3xs text-brand-300 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded px-0.5"
                  title={`Ir para job pai ${resumedFrom.parentJobId}`}
                >
                  #{resumedFrom.parentJobId.slice(0, 8)}
                </button>
              </>
            )}
          </div>
        </div>
      )}

      {/* Relação de uso: geração a partir de um checkpoint */}
      {usedFrom && (
        <div className="rounded-lg border border-amber-400/30 bg-amber-500/10 p-2 text-2xs space-y-0.5">
          <span className="font-mono text-3xs uppercase tracking-caps text-amber-300">
            Usou modelo
          </span>
          <div className="flex items-center gap-1.5 flex-wrap text-zinc-200">
            <span className="font-medium truncate max-w-[200px]" title={usedFrom.checkpointLabel}>
              {usedFrom.checkpointLabel}
            </span>
            {usedFrom.parentJobId && (
              <>
                <span className="text-zinc-500">·</span>
                <button
                  type="button"
                  onClick={() => onSelectJob(usedFrom.parentJobId!)}
                  className="font-mono text-3xs text-amber-300 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded px-0.5"
                  title={`Ir para job pai ${usedFrom.parentJobId}`}
                >
                  #{usedFrom.parentJobId.slice(0, 8)}
                </button>
              </>
            )}
          </div>
        </div>
      )}

      {/* Checkpoints produzidos por este job */}
      {checkpoints.length > 0 && (
        <div className="space-y-2 pt-2 border-t border-white/5">
          <div className="flex items-center justify-between gap-2">
            <span className="font-mono text-3xs uppercase tracking-caps text-zinc-400">
              Checkpoints produzidos ({checkpoints.length})
            </span>
            {checkpoints.length > 5 && (
              <button
                type="button"
                onClick={() => setExpanded((v) => !v)}
                className="font-mono text-3xs text-brand-400 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 rounded px-0.5"
              >
                {expanded ? "recolher" : `+${checkpoints.length - 5} mais`}
              </button>
            )}
          </div>

          <div className="space-y-1.5">
            {displayedCheckpoints.map((cp) => (
              <CheckpointItem
                key={cp.node.id}
                info={cp}
                ownerJobId={rawJobId}
                loadedArtifactJobIds={loadedArtifactJobIds}
                onSelectJob={onSelectJob}
                onFocusArtifact={onFocusArtifact}
              />
            ))}
          </div>
        </div>
      )}

      {/* Gerações produzidas por este job */}
      {generations.length > 0 && (
        <div className="space-y-2 pt-2 border-t border-white/5">
          <span className="font-mono text-3xs uppercase tracking-caps text-zinc-400">
            Gerações ({generations.length})
          </span>
          <div className="space-y-1.5">
            {generations.map((gen) => (
              <div
                key={gen.id}
                className="flex items-center gap-1.5 rounded-lg border border-white/5 bg-white/[0.02] p-2 text-2xs text-zinc-300"
              >
                <IconSparkles className="size-3.5 text-zinc-400 shrink-0" />
                <span className="truncate">{gen.label}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * Seção recolhível de linhagem do job (fatia 5b): dataset de origem, cadeia
 * cronológica de jobs com seus checkpoints produzidos agrupados, conexões de
 * resume explicitadas em ambos os lados e gerações associadas.
 */
export function JobLineage({
  jobId,
  loadedArtifactJobIds,
  onSelectJob,
  onSelectDataset,
  onFocusArtifact,
}: JobLineageProps) {
  const [open, setOpen] = useState(false);
  const [graph, setGraph] = useState<LineageGraph | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const ctrl = new AbortController();
    setGraph(null);
    setLoading(true);
    setError(null);
    getJobLineage(jobId, { signal: ctrl.signal })
      .then((data) => {
        if (!ctrl.signal.aborted) setGraph(data);
      })
      .catch((err: unknown) => {
        if (!ctrl.signal.aborted) {
          if (err instanceof DOMException && err.name === "AbortError") return;
          setError("Falha ao carregar a linhagem deste job.");
        }
      })
      .finally(() => {
        if (!ctrl.signal.aborted) setLoading(false);
      });
    return () => {
      ctrl.abort();
    };
  }, [open, jobId]);

  const layout = graph ? buildLineageLayout(graph) : null;
  const currentNodeId = `job:${jobId}`;

  const isEmpty =
    !!layout &&
    layout.datasets.length === 0 &&
    (layout.jobGroups.length === 0 ||
      (layout.jobGroups.length === 1 &&
        layout.jobGroups[0].job.id === currentNodeId &&
        layout.jobGroups[0].checkpoints.length === 0 &&
        layout.jobGroups[0].generations.length === 0 &&
        !layout.jobGroups[0].resumedFrom)) &&
    layout.orphanCheckpoints.length === 0 &&
    layout.orphanGenerations.length === 0;

  return (
    <div className="space-y-3 pt-3 border-t border-white/10">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-controls={`job-lineage-${jobId}`}
        className="flex min-h-[32px] w-full items-center justify-between gap-2 rounded-lg px-1 py-1 text-left hover:bg-white/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
      >
        <span className="flex items-center gap-2 font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-300">
          <IconNetwork className="size-3.5 text-brand-300" />
          Linhagem
        </span>
        <IconChevronDown
          className={`size-3.5 text-zinc-400 transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>

      {open && (
        <div id={`job-lineage-${jobId}`} className="space-y-4">
          {loading && (
            <div className="flex items-center gap-2 text-2xs text-zinc-400 px-1">
              <Spinner className="size-3.5" />
              Carregando linhagem…
            </div>
          )}

          {!loading && error && (
            <div className="flex items-center gap-2 rounded-lg border border-status-alert/30 bg-status-alert/10 px-3 py-2 text-2xs text-amber-300">
              <IconAlertTriangle className="size-3.5 shrink-0" />
              {error}
            </div>
          )}

          {!loading && !error && layout && isEmpty && (
            <p className="px-1 text-2xs text-zinc-500">
              Este job não tem dataset, ancestrais ou descendentes rastreados.
            </p>
          )}

          {!loading && !error && layout && !isEmpty && (
            <div className="space-y-4">
              {/* Datasets de Origem */}
              {layout.datasets.length > 0 && (
                <div className="space-y-2">
                  <h4 className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-500">
                    Dataset de Origem
                  </h4>
                  <div className="flex flex-wrap gap-2">
                    {layout.datasets.map((ds) => (
                      <button
                        key={ds.id}
                        type="button"
                        onClick={() => onSelectDataset(stripPrefix(ds.id))}
                        className="group flex items-center gap-2 rounded-xl border border-white/10 bg-white/[0.02] px-3 py-2 text-left transition-colors hover:border-white/20 hover:bg-white/[0.05] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
                        title={`Abrir dataset ${ds.label}`}
                      >
                        <IconDatabase className="size-3.5 text-brand-300 shrink-0" />
                        <span className="text-xs font-semibold text-zinc-100">
                          {ds.label}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}

              {/* Cadeia de Treinos / Execuções */}
              {layout.jobGroups.length > 0 && (
                <div className="space-y-3">
                  <h4 className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-500">
                    Cadeia de Treinos & Modelos
                  </h4>
                  <div className="space-y-3">
                    {layout.jobGroups.map((group) => (
                      <JobGroupCard
                        key={group.job.id}
                        group={group}
                        currentJobId={jobId}
                        loadedArtifactJobIds={loadedArtifactJobIds}
                        onSelectJob={onSelectJob}
                        onFocusArtifact={onFocusArtifact}
                      />
                    ))}
                  </div>
                </div>
              )}

              {/* Checkpoints ou gerações órfãos (Outros artefatos) */}
              {(layout.orphanCheckpoints.length > 0 ||
                layout.orphanGenerations.length > 0) && (
                <div className="space-y-2 pt-2 border-t border-white/5">
                  <h4 className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-500">
                    Outros Artefatos
                  </h4>
                  <div className="space-y-1.5">
                    {layout.orphanCheckpoints.map((cp) => (
                      <div
                        key={cp.node.id}
                        className="rounded-lg border border-white/5 bg-white/[0.02] p-2 text-xs text-zinc-300"
                      >
                        {cp.node.label}
                      </div>
                    ))}
                    {layout.orphanGenerations.map((gen) => (
                      <div
                        key={gen.id}
                        className="flex items-center gap-1.5 rounded-lg border border-white/5 bg-white/[0.02] p-2 text-2xs text-zinc-300"
                      >
                        <IconSparkles className="size-3.5 text-zinc-400 shrink-0" />
                        <span className="truncate">{gen.label}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
