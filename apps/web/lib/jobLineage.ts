import type { LineageGraph, LineageNode } from "@/types/studio";

export interface LineageColumn {
  kind: LineageNode["kind"];
  title: string;
  nodes: LineageNode[];
}

export interface LineageLayout {
  columns: LineageColumn[];
  nodeById: Map<string, LineageNode>;
  /** Id do job pai (via checkpoint: job→checkpoint→job), ou null quando não há pai. */
  parentOf: Map<string, string | null>;
}

const COLUMN_ORDER: { kind: LineageNode["kind"]; title: string }[] = [
  { kind: "dataset", title: "Dataset" },
  { kind: "job", title: "Jobs" },
  { kind: "checkpoint", title: "Checkpoints" },
  { kind: "generation", title: "Gerações" },
];

function sortChrono(nodes: LineageNode[]): LineageNode[] {
  return [...nodes].sort((a, b) => {
    const ta = a.createdAt ? Date.parse(a.createdAt) : NaN;
    const tb = b.createdAt ? Date.parse(b.createdAt) : NaN;
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

/** Monta as colunas (dataset → jobs → checkpoints → gerações) em ordem cronológica. */
export function buildLineageLayout(graph: LineageGraph): LineageLayout {
  const nodeById = new Map(graph.nodes.map((n) => [n.id, n]));
  const parentOf = deriveParents(graph);
  const columns = COLUMN_ORDER.map(({ kind, title }) => ({
    kind,
    title,
    nodes: sortChrono(graph.nodes.filter((n) => n.kind === kind)),
  })).filter((col) => col.nodes.length > 0);
  return { columns, nodeById, parentOf };
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
