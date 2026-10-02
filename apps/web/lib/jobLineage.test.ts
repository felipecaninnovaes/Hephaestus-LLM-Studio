import { describe, expect, it } from "bun:test";
import { ancestorChain, buildLineageLayout } from "@/lib/jobLineage";
import type { LineageGraph } from "@/types/studio";

function node(
  id: string,
  kind: "dataset" | "job" | "checkpoint" | "generation",
  extra: Partial<{ label: string; createdAt: string; epoch: number }> = {},
) {
  return {
    id,
    kind,
    label: extra.label ?? id,
    createdAt: extra.createdAt ?? null,
    epoch: extra.epoch ?? null,
  };
}

describe("buildLineageLayout", () => {
  it("agrupa em colunas dataset → jobs → checkpoints, em ordem cronológica", () => {
    const graph: LineageGraph = {
      nodes: [
        node("dataset:boys", "dataset", { label: "boys" }),
        node("job:a", "job", { createdAt: "2026-01-01T00:00:00Z" }),
        node("checkpoint:a5", "checkpoint", { epoch: 5 }),
      ],
      edges: [
        { from: "dataset:boys", to: "job:a", kind: "trains" },
        { from: "job:a", to: "checkpoint:a5", kind: "produced" },
      ],
    };
    const layout = buildLineageLayout(graph);
    expect(layout.columns.map((c) => c.kind)).toEqual([
      "dataset",
      "job",
      "checkpoint",
    ]);
    expect(layout.columns[0].nodes[0].id).toBe("dataset:boys");
  });

  it("deriva a cadeia de 2 resumes (job → checkpoint → job → checkpoint → job)", () => {
    const graph: LineageGraph = {
      nodes: [
        node("job:a", "job"),
        node("checkpoint:a5", "checkpoint", { epoch: 5 }),
        node("job:b", "job"),
        node("checkpoint:b2", "checkpoint", { epoch: 2 }),
        node("job:c", "job"),
      ],
      edges: [
        { from: "job:a", to: "checkpoint:a5", kind: "produced" },
        { from: "checkpoint:a5", to: "job:b", kind: "resumed_by" },
        { from: "job:b", to: "checkpoint:b2", kind: "produced" },
        { from: "checkpoint:b2", to: "job:c", kind: "resumed_by" },
      ],
    };
    const layout = buildLineageLayout(graph);
    expect(layout.parentOf.get("job:c")).toBe("job:b");
    expect(layout.parentOf.get("job:b")).toBe("job:a");
    expect(layout.parentOf.get("job:a")).toBeNull();
    expect(ancestorChain(layout.parentOf, "job:c")).toEqual([
      "job:a",
      "job:b",
      "job:c",
    ]);
  });

  it("job sem pai (treino do zero) tem parentOf null", () => {
    const graph: LineageGraph = {
      nodes: [node("dataset:boys", "dataset"), node("job:a", "job")],
      edges: [{ from: "dataset:boys", to: "job:a", kind: "trains" }],
    };
    const layout = buildLineageLayout(graph);
    expect(layout.parentOf.get("job:a")).toBeNull();
    expect(ancestorChain(layout.parentOf, "job:a")).toEqual(["job:a"]);
  });

  it("nó órfão (sem arestas) ainda aparece em sua coluna de kind", () => {
    const graph: LineageGraph = {
      nodes: [
        node("job:a", "job"),
        node("generation:orphan", "generation", { label: "prompt perdido" }),
      ],
      edges: [],
    };
    const layout = buildLineageLayout(graph);
    const genCol = layout.columns.find((c) => c.kind === "generation");
    expect(genCol?.nodes.map((n) => n.id)).toEqual(["generation:orphan"]);
  });

  it("ciclo defensivo não trava ancestorChain", () => {
    // Dados inconsistentes/corrompidos: job:a aponta (via checkpoint) de volta
    // para um descendente, formando um ciclo. ancestorChain deve retornar em
    // tempo finito em vez de loop infinito.
    const graph: LineageGraph = {
      nodes: [
        node("job:a", "job"),
        node("checkpoint:a1", "checkpoint"),
        node("job:b", "job"),
        node("checkpoint:b1", "checkpoint"),
      ],
      edges: [
        { from: "job:a", to: "checkpoint:a1", kind: "produced" },
        { from: "checkpoint:a1", to: "job:b", kind: "resumed_by" },
        { from: "job:b", to: "checkpoint:b1", kind: "produced" },
        { from: "checkpoint:b1", to: "job:a", kind: "resumed_by" },
      ],
    };
    const layout = buildLineageLayout(graph);
    const chain = ancestorChain(layout.parentOf, "job:a");
    expect(chain.length).toBeGreaterThan(0);
    expect(new Set(chain).size).toBe(chain.length);
  });
});
