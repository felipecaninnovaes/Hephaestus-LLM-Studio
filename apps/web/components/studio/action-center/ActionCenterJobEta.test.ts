import { describe, expect, it } from "bun:test";
import { isRunningTrainingJob, trainingEtaLabel } from "./ActionCenterJobEta";

describe("trainingEtaLabel", () => {
  it("usa etaFormatted do servidor", () => {
    expect(trainingEtaLabel("1m 4s", 64)).toBe("≈ 1m 4s restantes");
  });
  it("remove prefixo ETA", () => {
    expect(trainingEtaLabel("ETA ~2m", null)).toBe("≈ 2m restantes");
  });
  it("cai em etaSeconds sem string", () => {
    expect(trainingEtaLabel(null, 64)).toBe("≈ 1m 4s restantes");
  });
  it("null sem dado ou ETA zerado", () => {
    expect(trainingEtaLabel(null, null)).toBeNull();
    expect(trainingEtaLabel("", 0)).toBeNull();
  });
});

describe("isRunningTrainingJob", () => {
  const base = { engine: "diffusion", mode: "train", status: "running" } as const;
  it("treino difusão/yolo running", () => {
    expect(isRunningTrainingJob({ ...base, kind: "diffusion" } as never)).toBe(true);
    expect(isRunningTrainingJob({ ...base, engine: "yolo", kind: "yolo_train" } as never)).toBe(true);
  });
  it("exclui geração, predição e não-running", () => {
    expect(isRunningTrainingJob({ ...base, kind: "diffusion_generate" } as never)).toBe(false);
    expect(isRunningTrainingJob({ ...base, kind: "diffusion", mode: "generate" } as never)).toBe(false);
    expect(isRunningTrainingJob({ ...base, kind: "yolo_predict", engine: "yolo" } as never)).toBe(false);
    expect(isRunningTrainingJob({ ...base, kind: "diffusion", status: "done" } as never)).toBe(false);
  });
});
