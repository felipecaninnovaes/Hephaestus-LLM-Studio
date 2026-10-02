import type { LineageGraph, LineageNode } from "@/types/studio";

export interface CheckpointInfo {
  node: LineageNode;
  /** Id (sem prefixo) do job que retomou deste checkpoint (resumed_by), se houver */
  resumedByJobId?: string | null;
  /** Id (sem prefixo) do job/geração que usou este checkpoint (used_by), se houver */
  usedByJobId?: string | null;
}

export interface JobGroup {
  job: LineageNode;
  checkpoints: CheckpointInfo[];
  generations: LineageNode[];
  /** Se este job foi retomado de um checkpoint: metadados do checkpoint de origem e do job pai */
  resumedFrom?: {
    checkpointId: string;
    checkpointLabel: string;
    parentJobId?: string | null;
  } | null;
  /** Se este job (ex.: geração) usou um checkpoint: metadados de origem */
  usedFrom?: {
    checkpointId: string;
    checkpointLabel: string;
    parentJobId?: string | null;
  } | null;
}

export interface LineageLayout {
  datasets: LineageNode[];
  jobGroups: JobGroup[];
  orphanCheckpoints: CheckpointInfo[];
  orphanGenerations: LineageNode[];
  nodeById: Map<string, LineageNode>;
  /** Id do job pai (via checkpoint: job→checkpoint→job), ou null quando não há pai. */
  parentOf: Map<string, string | null>;
}

function parseTime(iso?: string | null): number {
  if (!iso) return NaN;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? NaN : t;
}

function sortChrono(nodes: LineageNode[]): LineageNode[] {
  return [...nodes].sort((a, b) => {
    const ta = parseTime(a.createdAt);
    const tb = parseTime(b.createdAt);
    if (!Number.isNaN(ta) && !Number.isNaN(tb) && ta !== tb) return ta - tb;
    if (a.epoch != null && b.epoch != null && a.epoch !== b.epoch)
      return a.epoch - b.epoch;
    return a.label.localeCompare(b.label);
  });
}

/**
 * Deriva, para cada nó `kind: job`, o id do job pai via o caminho
 * job→checkpoint→job (não há aresta direta job→job no grafo). Um job
 * resumido/gerado a partir de um checkpoint tem como pai o job que produziu
 * aquele checkpoint. Sem checkpoint de origem ou sem produtor ⇒ pai `null`.
 */
function deriveParents(graph: LineageGraph): Map<string, string | null> {
  const producedBy = new Map<string, string>(); // checkpoint/generation id -> job que produziu
  const originCheckpoint = new Map<string, string>(); // job id -> checkpoint de origem

  for (const edge of graph.edges) {
    if (edge.kind === "produced") {
      producedBy.set(edge.to, edge.from);
    } else if (edge.kind === "resumed_by" || edge.kind === "used_by") {
      originCheckpoint.set(edge.to, edge.from);
    }
  }

  const parentOf = new Map<string, string | null>();
  for (const node of graph.nodes) {
    if (node.kind !== "job") continue;
    const checkpointId = originCheckpoint.get(node.id);
    const parentJobId = checkpointId ? producedBy.get(checkpointId) : undefined;
    parentOf.set(node.id, parentJobId ?? null);
  }
  return parentOf;
}

/**
 * Monta a estrutura da linhagem agrupada por Job:
 * - Datasets de origem
 * - JobGroups na ordem cronológica de jobs, com seus checkpoints produzidos (e quem os retomou)
 * - Checkpoints/gerações órfãos (se houver, sem produtor conhecido)
 */
export function buildLineageLayout(graph: LineageGraph): LineageLayout {
  const nodeById = new Map(graph.nodes.map((n) => [n.id, n]));
  const parentOf = deriveParents(graph);

  // Mapeamentos de arestas
  const producedBy = new Map<string, string>(); // artifactId -> jobId
  const resumedBy = new Map<string, string>(); // checkpointId -> jobId
  const usedBy = new Map<string, string>(); // checkpointId -> jobId
  const resumedFromMap = new Map<string, string>(); // jobId -> checkpointId
  const usedFromMap = new Map<string, string>(); // jobId -> checkpointId

  for (const edge of graph.edges) {
    if (edge.kind === "produced") {
      producedBy.set(edge.to, edge.from);
    } else if (edge.kind === "resumed_by") {
      resumedBy.set(edge.from, edge.to);
      resumedFromMap.set(edge.to, edge.from);
    } else if (edge.kind === "used_by") {
      usedBy.set(edge.from, edge.to);
      usedFromMap.set(edge.to, edge.from);
    }
  }

  const datasets = sortChrono(graph.nodes.filter((n) => n.kind === "dataset"));
  const jobs = sortChrono(graph.nodes.filter((n) => n.kind === "job"));

  // Checkpoints agrupados por produtor
  const checkpointsByJob = new Map<string, LineageNode[]>();
  const generationsByJob = new Map<string, LineageNode[]>();
  const orphanCheckpointsList: LineageNode[] = [];
  const orphanGenerationsList: LineageNode[] = [];

  for (const node of graph.nodes) {
    if (node.kind === "checkpoint") {
      const producer = producedBy.get(node.id);
      if (producer) {
        const list = checkpointsByJob.get(producer) ?? [];
        list.push(node);
        checkpointsByJob.set(producer, list);
      } else {
        orphanCheckpointsList.push(node);
      }
    } else if (node.kind === "generation") {
      const producer = producedBy.get(node.id);
      if (producer) {
        const list = generationsByJob.get(producer) ?? [];
        list.push(node);
        generationsByJob.set(producer, list);
      } else {
        orphanGenerationsList.push(node);
      }
    }
  }

  const jobGroups: JobGroup[] = jobs.map((job) => {
    const rawCheckpoints = sortChrono(checkpointsByJob.get(job.id) ?? []);
    const checkpoints: CheckpointInfo[] = rawCheckpoints.map((cp) => {
      const resId = resumedBy.get(cp.id);
      const useId = usedBy.get(cp.id);
      return {
        node: cp,
        resumedByJobId: resId ? (resId.includes(":") ? resId.split(":").slice(1).join(":") : resId) : null,
        usedByJobId: useId ? (useId.includes(":") ? useId.split(":").slice(1).join(":") : useId) : null,
      };
    });

    const generations = sortChrono(generationsByJob.get(job.id) ?? []);

    let resumedFrom: JobGroup["resumedFrom"] = null;
    const originCpId = resumedFromMap.get(job.id);
    if (originCpId) {
      const cpNode = nodeById.get(originCpId);
      const parentJobFullId = parentOf.get(job.id);
      const rawOrigin = originCpId.includes(":") ? originCpId.split(":").slice(1).join(":") : originCpId;
      const rawParent = parentJobFullId
        ? (parentJobFullId.includes(":") ? parentJobFullId.split(":").slice(1).join(":") : parentJobFullId)
        : null;
      resumedFrom = {
        checkpointId: rawOrigin,
        checkpointLabel: cpNode?.label ?? originCpId,
        parentJobId: rawParent,
      };
    }

    let usedFrom: JobGroup["usedFrom"] = null;
    const originUsedCpId = usedFromMap.get(job.id);
    if (originUsedCpId) {
      const cpNode = nodeById.get(originUsedCpId);
      const parentJobFullId = parentOf.get(job.id);
      const rawOrigin = originUsedCpId.includes(":") ? originUsedCpId.split(":").slice(1).join(":") : originUsedCpId;
      const rawParent = parentJobFullId
        ? (parentJobFullId.includes(":") ? parentJobFullId.split(":").slice(1).join(":") : parentJobFullId)
        : null;
      usedFrom = {
        checkpointId: rawOrigin,
        checkpointLabel: cpNode?.label ?? originUsedCpId,
        parentJobId: rawParent,
      };
    }

    return {
      job,
      checkpoints,
      generations,
      resumedFrom,
      usedFrom,
    };
  });

  const orphanCheckpoints: CheckpointInfo[] = sortChrono(orphanCheckpointsList).map((cp) => {
    const resId = resumedBy.get(cp.id);
    const useId = usedBy.get(cp.id);
    return {
      node: cp,
      resumedByJobId: resId ? (resId.includes(":") ? resId.split(":").slice(1).join(":") : resId) : null,
      usedByJobId: useId ? (useId.includes(":") ? useId.split(":").slice(1).join(":") : useId) : null,
    };
  });

  const orphanGenerations = sortChrono(orphanGenerationsList);

  return {
    datasets,
    jobGroups,
    orphanCheckpoints,
    orphanGenerations,
    nodeById,
    parentOf,
  };
}

/**
 * Sobe a cadeia de pais de um job (via `parentOf`) até a raiz, com guarda de
 * ciclo (visited set) — nunca trava mesmo com dados inconsistentes/cíclicos.
 * Retorna do mais antigo (raiz) para o mais recente (o próprio `jobId`).
 */
export function ancestorChain(
  parentOf: Map<string, string | null>,
  jobId: string,
): string[] {
  const chain: string[] = [];
  const visited = new Set<string>();
  let current: string | null | undefined = jobId;
  while (current && !visited.has(current)) {
    visited.add(current);
    chain.push(current);
    current = parentOf.get(current) ?? null;
  }
  return chain.reverse();
}
