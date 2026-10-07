import type { DiffusionOptimizer } from "@/types/studio";

export const DIFFUSION_EPOCHS_MIN = 1;
export const DIFFUSION_EPOCHS_MAX = 100;

export type DiffusionBaseModel =
  | "sdxl"
  | "flux"
  | "flux-2-klein-9b"
  | "sd15"
  | "qwen-image-2.1";

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
 * Klein 9B: estimativa do LOOP de treino (4-bit ≈ 8,8 GB a 1024; teto medido
 * do loop na RTX 3060: 7,4–9,0 GiB). O pico do precompute de embeds do text
 * encoder (~11,9 GiB) é adaptativo (OOM ⇒ batch 32→2 e segue) e NÃO entra;
 * tiers 9B: ~9 GB (2/4-bit), ~20 GB (6/8-bit), ~28 GB (sem quantização).
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
  } else if (baseModel === "flux-2-klein-9b") {
    baseGb =
      quantization === "2bit"
        ? 8.4
        : quantization === "4bit"
          ? 8.8
          : quantization === "6bit"
            ? 19.0
            : quantization === "8bit"
              ? 19.8
              : 27.8;
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

  // Ajuste por resolução relativa a 1024 (9B: base é o loop de treino a 1024)
  if (baseModel === "flux-2-klein-9b") {
    if (resolution <= 512) baseGb -= 2.0;
    else if (resolution <= 768) baseGb -= 0.8;
    else if (resolution > 1024) baseGb += 1.0;
  } else if (resolution <= 512) {
    baseGb -= baseModel === "sd15" ? 2.0 : 3.0;
  } else if (resolution <= 768) {
    baseGb -= baseModel === "sd15" ? 1.0 : 1.5;
  }

  const batchMemory =
    (batchSize - 1) *
    (baseModel === "flux-2-klein-9b"
      ? 2.5
      : baseModel === "flux" || baseModel === "qwen-image-2.1"
        ? 1.8
        : baseModel === "sdxl"
          ? 2.0
          : 1.2);
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
