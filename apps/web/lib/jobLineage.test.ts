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
  it("agrupa checkpoints sob o job que os produziu e marca resumed_by / resumedFrom", () => {
    const graph: LineageGraph = {
      nodes: [
        node("dataset:boys", "dataset", { label: "boys" }),
        node("job:parent", "job", {
          label: "train flux",
          createdAt: "2026-09-30T00:00:00Z",
        }),
        node("checkpoint:cp1", "checkpoint", {
          label: "adapter_epoch_005.safetensors",
          epoch: 5,
        }),
        node("checkpoint:cp2", "checkpoint", {
          label: "adapter_epoch_006.safetensors",
          epoch: 6,
        }),
        node("job:child", "job", {
          label: "train flux",
          createdAt: "2026-10-01T00:00:00Z",
        }),
      ],
      edges: [
        { from: "dataset:boys", to: "job:parent", kind: "trains" },
        { from: "job:parent", to: "checkpoint:cp1", kind: "produced" },
        { from: "job:parent", to: "checkpoint:cp2", kind: "produced" },
        { from: "checkpoint:cp1", to: "job:child", kind: "resumed_by" },
      ],
    };

    const layout = buildLineageLayout(graph);

    expect(layout.datasets).toHaveLength(1);
    expect(layout.datasets[0].id).toBe("dataset:boys");

    expect(layout.jobGroups).toHaveLength(2);

    const [parentGroup, childGroup] = layout.jobGroups;
    expect(parentGroup.job.id).toBe("job:parent");
    expect(parentGroup.checkpoints).toHaveLength(2);
    expect(parentGroup.resumedFrom).toBeNull();

    // Checkpoint cp1 foi retomado pelo child
    const cp1 = parentGroup.checkpoints.find((c) => c.node.id === "checkpoint:cp1");
    expect(cp1?.resumedByJobId).toBe("child");

    // Child group foi retomado de cp1 produzido por parent
    expect(childGroup.job.id).toBe("job:child");
    expect(childGroup.resumedFrom).toEqual({
      checkpointId: "cp1",
      checkpointLabel: "adapter_epoch_005.safetensors",
      parentJobId: "parent",
    });
  });

  it("deriva a cadeia de 2 resumes (job → checkpoint → job → checkpoint → job)", () => {
    const graph: LineageGraph = {
      nodes: [
        node("job:a", "job", { createdAt: "2026-09-29T00:00:00Z" }),
        node("checkpoint:cp-a", "checkpoint"),
        node("job:b", "job", { createdAt: "2026-09-30T00:00:00Z" }),
        node("checkpoint:cp-b", "checkpoint"),
        node("job:c", "job", { createdAt: "2026-10-01T00:00:00Z" }),
      ],
      edges: [
        { from: "job:a", to: "checkpoint:cp-a", kind: "produced" },
        { from: "checkpoint:cp-a", to: "job:b", kind: "resumed_by" },
        { from: "job:b", to: "checkpoint:cp-b", kind: "produced" },
        { from: "checkpoint:cp-b", to: "job:c", kind: "resumed_by" },
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

  it("job sem pai (treino do zero) tem parentOf null e resumedFrom null", () => {
    const graph: LineageGraph = {
      nodes: [node("job:a", "job")],
      edges: [],
    };
    const layout = buildLineageLayout(graph);
    expect(layout.parentOf.get("job:a")).toBeNull();
    expect(layout.jobGroups[0].resumedFrom).toBeNull();
    expect(ancestorChain(layout.parentOf, "job:a")).toEqual(["job:a"]);
  });

  it("nó órfão (sem produtor) aparece em orphanCheckpoints", () => {
    const graph: LineageGraph = {
      nodes: [
        node("checkpoint:orphan-cp", "checkpoint", { label: "standalone.pt" }),
      ],
      edges: [],
    };
    const layout = buildLineageLayout(graph);
    expect(layout.orphanCheckpoints).toHaveLength(1);
    expect(layout.orphanCheckpoints[0].node.id).toBe("checkpoint:orphan-cp");
  });

  it("ciclo defensivo não trava ancestorChain", () => {
    const parentOf = new Map<string, string | null>([
      ["job:a", "job:b"],
      ["job:b", "job:c"],
      ["job:c", "job:a"],
    ]);
    const chain = ancestorChain(parentOf, "job:a");
    expect(chain.length).toBeGreaterThan(0);
    expect(chain.length).toBeLessThanOrEqual(3);
  });
});
