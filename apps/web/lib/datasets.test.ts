import { describe, expect, it } from "bun:test";
import {
  canTrainDataset,
  trainDatasetActionLabel,
  trainDatasetDisabledReason,
} from "./datasets";
import type { Dataset } from "@/types/studio";

function createMockDataset(overrides: Partial<Dataset> = {}): Dataset {
  return {
    id: "ds-1",
    slug: "dataset-test",
    title: "Dataset Teste",
    category: "yolo",
    type: "yolo_bbox",
    task: "detect_track",
    format: "yolo_txt",
    status: "ready",
    source: null,
    sizeBytes: 1024,
    imagesCount: 10,
    labeledCount: 10,
    classes: [{ id: "c1", name: "person", idx: 0, color: "#ff0000" }],
    autoTracked: false,
    trashCount: 0,
    createdAt: "2026-01-01T00:00:00Z",
    lastModified: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("Unified Dataset Training", () => {
  describe("canTrainDataset", () => {
    it("allows training for YOLO datasets with >= 1 class and >= 1 image", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 5, classes: [{ id: "c1", name: "cat", idx: 0, color: "#fff" }] });
      expect(canTrainDataset(ds)).toBe(true);
    });

    it("disallows training for YOLO datasets with 0 classes", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 5, classes: [] });
      expect(canTrainDataset(ds)).toBe(false);
    });

    it("disallows training for YOLO datasets with 0 images", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 0, classes: [{ id: "c1", name: "cat", idx: 0, color: "#fff" }] });
      expect(canTrainDataset(ds)).toBe(false);
    });
    it("allows training for diffusion datasets with >= 1 image regardless of classes", () => {
      const ds = createMockDataset({ category: "difusao", imagesCount: 12, classes: [] });
      expect(canTrainDataset(ds)).toBe(true);

      const dsEnglish = createMockDataset({ category: "diffusion" as unknown as Dataset["category"], imagesCount: 3, classes: [] });
      expect(canTrainDataset(dsEnglish)).toBe(true);
    });

    it("disallows training for diffusion datasets with 0 images", () => {
      const ds = createMockDataset({ category: "difusao", imagesCount: 0, classes: [] });
      expect(canTrainDataset(ds)).toBe(false);
    });

    it("disallows training for unsupported categories", () => {
      const ds = createMockDataset({ category: "openclip", imagesCount: 20, classes: [] });
      expect(canTrainDataset(ds)).toBe(false);
    });
  });

  describe("trainDatasetDisabledReason", () => {
    it("returns helpful message for YOLO missing classes", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 5, classes: [] });
      expect(trainDatasetDisabledReason(ds)).toContain("≥1 classe");
    });

    it("returns helpful message for YOLO missing images", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 0, classes: [{ id: "c1", name: "cat", idx: 0, color: "#fff" }] });
      expect(trainDatasetDisabledReason(ds)).toContain("≥1 imagem");
    });

    it("returns modal open label when YOLO dataset is valid", () => {
      const ds = createMockDataset({ category: "yolo", imagesCount: 5, classes: [{ id: "c1", name: "cat", idx: 0, color: "#fff" }] });
      expect(trainDatasetDisabledReason(ds)).toBe("Abrir modal de treino YOLO");
    });

    it("returns helpful message for diffusion missing images", () => {
      const ds = createMockDataset({ category: "difusao", imagesCount: 0, classes: [] });
      expect(trainDatasetDisabledReason(ds)).toContain("≥1 imagem");
    });

    it("returns diffusion train label when diffusion dataset is valid", () => {
      const ds = createMockDataset({ category: "difusao", imagesCount: 5, classes: [] });
      expect(trainDatasetDisabledReason(ds)).toBe("Iniciar treino de difusão LoRA");
    });
  });

  describe("trainDatasetActionLabel", () => {
    it("returns 'Treinar YOLO' for YOLO datasets", () => {
      const ds = createMockDataset({ category: "yolo" });
      expect(trainDatasetActionLabel(ds)).toBe("Treinar YOLO");
    });

    it("returns 'Treinar Difusão' for diffusion datasets", () => {
      const dsDifusao = createMockDataset({ category: "difusao" });
      expect(trainDatasetActionLabel(dsDifusao)).toBe("Treinar Difusão");

      const dsDiffusion = createMockDataset({ category: "diffusion" as unknown as Dataset["category"] });
      expect(trainDatasetActionLabel(dsDiffusion)).toBe("Treinar Difusão");
    });
  });
});
