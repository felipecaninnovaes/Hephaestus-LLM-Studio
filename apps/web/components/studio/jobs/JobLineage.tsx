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
import { buildLineageLayout } from "@/lib/jobLineage";
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

interface NodeButtonProps {
  node: LineageNode;
  isCurrent: boolean;
  /** Id (sem prefixo) do job que produziu este nó, quando `kind: checkpoint`. */
  ownerJobId: string | null;
  loadedArtifactJobIds: Set<string>;
  onSelectJob: (jobId: string) => void;
  onSelectDataset: (datasetId: string) => void;
  onFocusArtifact: (jobId: string, artifactId: string) => void;
}

function NodeButton({
  node,
  isCurrent,
  ownerJobId,
  loadedArtifactJobIds,
  onSelectJob,
  onSelectDataset,
  onFocusArtifact,
}: NodeButtonProps) {
  const rawId = node.id.includes(":") ? node.id.split(":").slice(1).join(":") : node.id;

  const baseClasses =
    "group flex min-h-[32px] w-full flex-col items-start gap-0.5 rounded-xl border px-3 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-400/60";

  const toneClasses = isCurrent
    ? "border-brand-400/60 bg-brand-500/10"
    : "border-white/10 bg-white/[0.02] hover:bg-white/[0.05] hover:border-white/20";

  const header = (
    <div className="flex w-full items-center justify-between gap-2">
      <span className="truncate text-xs font-semibold text-zinc-100">
        {node.label}
      </span>
      {isCurrent && (
        <Badge variant="brand" className="shrink-0">
          atual
        </Badge>
      )}
    </div>
  );

  const meta: string[] = [];
  if (node.kind === "checkpoint" && node.epoch != null) {
    meta.push(`época ${node.epoch}`);
  }
  if (node.createdAt) {
    meta.push(new Date(node.createdAt).toLocaleString("pt-BR"));
  }
  if (node.kind !== "dataset") {
    meta.push(`#${idSuffix(node.id)}`);
  }

  const metaRow = meta.length > 0 && (
    <span className="font-mono text-3xs text-zinc-400">{meta.join(" · ")}</span>
  );

  if (node.kind === "job") {
    return (
      <button
        type="button"
        onClick={() => onSelectJob(rawId)}
        className={`${baseClasses} ${toneClasses}`}
        title={`Abrir job ${rawId}`}
      >
        {header}
        <div className="flex items-center gap-2">
          {node.status && (
            <Badge
              variant={jobStatusToBadgeVariant(node.status as JobStatus)}
              className="shrink-0"
            >
              {STATUS_LABEL[node.status as JobStatus] ?? node.status}
            </Badge>
          )}
          {metaRow}
        </div>
      </button>
    );
  }

  if (node.kind === "dataset") {
    return (
      <button
        type="button"
        onClick={() => onSelectDataset(rawId)}
        className={`${baseClasses} ${toneClasses}`}
        title={`Abrir dataset ${node.label}`}
      >
        <div className="flex w-full items-center gap-1.5">
          <IconDatabase className="size-3.5 text-brand-300 shrink-0" />
          {header}
        </div>
      </button>
    );
  }

  if (node.kind === "checkpoint") {
    const canFocus = !!ownerJobId && loadedArtifactJobIds.has(ownerJobId);
    if (canFocus && ownerJobId) {
      return (
        <button
          type="button"
          onClick={() => onFocusArtifact(ownerJobId, rawId)}
          className={`${baseClasses} ${toneClasses}`}
          title="Rolar até o artefato"
        >
          {header}
          {metaRow}
        </button>
      );
    }
    return (
      <div
        className={`${baseClasses} border-white/10 bg-white/[0.02] opacity-80`}
        aria-disabled="true"
      >
        {header}
        {metaRow}
      </div>
    );
  }

  // generation: sem rota própria no app ainda — exibido como informativo.
  return (
    <div className={`${baseClasses} border-white/10 bg-white/[0.02] opacity-80`}>
      <div className="flex w-full items-center gap-1.5">
        <IconSparkles className="size-3.5 text-zinc-400 shrink-0" />
        {header}
      </div>
      {metaRow}
    </div>
  );
}

/**
 * Seção recolhível de linhagem do job (fatia 5b): dataset de origem, cadeia
 * de jobs ancestrais/descendentes (pai via checkpoint), checkpoints com
 * época e gerações, em colunas cronológicas.
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
    let cancelled = false;
    setLoading(true);
    setError(null);
    getJobLineage(jobId)
      .then((data) => {
        if (!cancelled) setGraph(data);
      })
      .catch(() => {
        if (!cancelled) setError("Falha ao carregar a linhagem deste job.");
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, jobId]);

  const currentNodeId = `job:${jobId}`;
  const layout = graph ? buildLineageLayout(graph) : null;
  const isEmpty =
    !!layout &&
    layout.columns.every(
      (col) =>
        col.nodes.length === 0 ||
        (col.nodes.length === 1 && col.nodes[0].id === currentNodeId),
    );

  // checkpoint id -> id (sem prefixo) do job que o produziu (aresta "produced").
  const checkpointOwner = new Map<string, string>();
  if (graph) {
    for (const edge of graph.edges) {
      if (edge.kind === "produced" && edge.to.startsWith("checkpoint:")) {
        const owner = edge.from.startsWith("job:")
          ? edge.from.slice("job:".length)
          : edge.from;
        checkpointOwner.set(edge.to, owner);
      }
    }
  }

  return (
    <div className="space-y-3 pt-3 border-t border-white/10">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-controls={`job-lineage-${jobId}`}
        className="flex w-full items-center justify-between gap-2 rounded-lg px-1 py-1 text-left hover:bg-white/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-400/60"
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
        <div id={`job-lineage-${jobId}`} className="space-y-3">
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
            <div className="flex gap-3 overflow-x-auto pb-1">
              {layout.columns.map((col) => (
                <div key={col.kind} className="min-w-[180px] flex-1 space-y-2">
                  <h4 className="font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-500">
                    {col.title}
                  </h4>
                  <div className="space-y-2">
                    {col.nodes.map((node) => (
                      <NodeButton
                        key={node.id}
                        node={node}
                        isCurrent={node.id === currentNodeId}
                        ownerJobId={checkpointOwner.get(node.id) ?? null}
                        loadedArtifactJobIds={loadedArtifactJobIds}
                        onSelectJob={onSelectJob}
                        onSelectDataset={onSelectDataset}
                        onFocusArtifact={onFocusArtifact}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
