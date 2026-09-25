import type { DiffusionOptimizer } from "@/types/studio";

export const DIFFUSION_EPOCHS_MIN = 1;
export const DIFFUSION_EPOCHS_MAX = 100;

export type DiffusionBaseModel = "sdxl" | "flux" | "sd15" | "qwen-image-2.1";

export interface DiffusionHyperparametersValues {
  baseModel: DiffusionBaseModel;
  triggerWord: string;
  epochs: number;
  batchSize: number;
  learningRate: string;
  rank: number;
  alpha: number;
}

/**
 * Estimativa preditiva de VRAM em GB para treino de difusão LoRA.
 * Base: SD1.5 (8 GB), SDXL (12 GB), Flux (16 GB), Qwen-Image-2.1 (16 GB).
 * Fator de Batch, Rank, Resolução e Otimizador adicionam overhead ou economia.
 */
export function estimateDiffusionVramGb(
  baseModel: DiffusionBaseModel,
  batchSize: number,
  rank: number,
  resolution: number = 1024,
  optimizer: DiffusionOptimizer = "paged_adamw8bit",
  mixedPrecision: "fp16" | "bf16" | "no" = "fp16",
  quantization: "none" | "2bit" | "4bit" | "6bit" | "8bit" = "4bit",
): number {
  let baseGb = 12.0;
  if (baseModel === "sd15") {
    baseGb =
      quantization === "2bit"
        ? 4.5
        : quantization === "4bit"
          ? 5.0
          : quantization === "6bit"
            ? 5.5
            : quantization === "8bit"
              ? 6.0
              : 7.5;
  } else if (baseModel === "sdxl") {
    baseGb =
      quantization === "2bit"
        ? 7.0
        : quantization === "4bit"
          ? 8.0
          : quantization === "6bit"
            ? 9.0
            : quantization === "8bit"
              ? 10.0
              : 12.0;
  } else if (baseModel === "flux") {
    baseGb =
      quantization === "2bit"
        ? 7.5
        : quantization === "4bit"
          ? 8.0
          : quantization === "6bit"
            ? 10.5
            : quantization === "8bit"
              ? 12.5
              : 20.0;
  } else if (baseModel === "qwen-image-2.1") {
    baseGb =
      quantization === "2bit"
        ? 8.0
        : quantization === "4bit"
          ? 9.5
          : quantization === "6bit"
            ? 11.5
            : quantization === "8bit"
              ? 13.5
              : 18.0;
  }

  // Ajuste por resolução relativa a 1024
  if (resolution <= 512) {
    baseGb -= baseModel === "sd15" ? 2.0 : 3.0;
  } else if (resolution <= 768) {
    baseGb -= baseModel === "sd15" ? 1.0 : 1.5;
  }

  const batchMemory =
    (batchSize - 1) *
    (baseModel === "flux" || baseModel === "qwen-image-2.1" ? 1.8 : baseModel === "sdxl" ? 2.0 : 1.2);
  const rankMemory = (rank / 64) * 0.8;
  const optimMemory =
    optimizer === "paged_adamw8bit"
      ? -0.2
      : optimizer === "adamw8bit"
        ? 0
        : optimizer === "prodigy"
          ? 0.6
          : optimizer === "paged_adamw32bit"
            ? 0.8
            : 1.5;
  const precMemory = mixedPrecision === "no" ? 3.5 : 0;

  return Math.max(
    4.0,
    Math.round(
      (baseGb + batchMemory + rankMemory + optimMemory + precMemory) * 10,
    ) / 10,
  );
}
