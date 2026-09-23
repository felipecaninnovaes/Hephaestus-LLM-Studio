export type DiffusionOptimizer =
  | "adamw8bit"
  | "adamw"
  | "prodigy"
  | "paged_adamw8bit"
  | "paged_adamw32bit";

export interface DiffusionJobRequest {
  datasetId: string;
  /** "flux" (FLUX.2 Klein 4B), "sdxl" (SDXL 1.0) ou "sd15" (Stable Diffusion 1.5). XOR com customModelId; ambos ausentes ⇒ "sdxl" (retrocompat). */
  baseModel?: "sdxl" | "flux" | "sd15" | "qwen-image-2.1";
  /** UUID de checkpoint custom (kind=checkpoint). XOR com baseModel. */
  customModelId?: string | null;
  /** UUID de text encoder custom (kind=text_encoder). Só vale p/ arch flux-2-klein-4b; omitido = encoder oficial BFL. */
  textEncoderModelId?: string | null;
  triggerWord?: string;
  epochs?: number;
  batchSize?: number;
  learningRate?: number;
  rank?: number;
  alpha?: number;
  weights?: string | null;
  orchestratorId?: string | null;
  samplePrompt?: string;
  sampleInterval?: number;
  sampleSeed?: number;
  resolution?: number;
  gradientAccumulationSteps?: number;
  optimizer?: DiffusionOptimizer;
  lrScheduler?: "cosine" | "linear" | "constant" | "constant_with_warmup";
  lrWarmupSteps?: number;
  mixedPrecision?: "fp16" | "bf16" | "no";
  quantization?: "none" | "2bit" | "4bit" | "6bit" | "8bit";
  /** Dataset de regularização/controle (Motor Flux.2): opcional, UUID. */
  controlDatasetId?: string | null;
  /** Pré-computa embeddings das captions (acelera datasets grandes). Padrão false. */
  cacheTextEmbeddings?: boolean;
  /** Bucketing por aspect ratio: preserva a proporção das imagens (padrão true). */
  enableBucket?: boolean;
  checkpointInterval?: number;
  epochOffset?: number;
  outputName?: string | null;
}

export interface DiffusionPreset {
  name: string;
  description?: string;
  version?: string;
  baseModel: "sdxl" | "flux" | "sd15" | "qwen-image-2.1";
  triggerWord?: string;
  epochs: number;
  batchSize: number;
  learningRate: string;
  rank: number;
  alpha: number;
  resolution?: number;
  gradientAccumulationSteps?: number;
  optimizer?: DiffusionOptimizer;
  lrScheduler?: "cosine" | "linear" | "constant" | "constant_with_warmup";
  lrWarmupSteps?: number;
  mixedPrecision?: "fp16" | "bf16" | "no";
  quantization?: "none" | "2bit" | "4bit" | "6bit" | "8bit";
  controlDatasetId?: string | null;
  cacheTextEmbeddings?: boolean;
  enableBucket?: boolean;
  checkpointInterval?: number;
  epochOffset?: number;
  enableSamples?: boolean;
  samplePrompt?: string;
  sampleInterval?: number;
  sampleSeed?: string;
}

export function diffusionErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros do treino de difusão inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto — exige ≥1 imagem para treino.";
    case "queue_unavailable":
      return "Fila de treino indisponível — tente novamente.";
    case "not_found":
      return "Job ou modelo não encontrado.";
    default:
      return "Falha ao criar job de treino de difusão.";
  }
}

export interface DiffusionGenerateJobRequest {
  baseModel?: "flux-2-klein-4b" | "sdxl" | "sd15" | "qwen-image-2.1";
  customModelId?: string | null;
  /** UUID de text encoder custom (kind=text_encoder). Só vale p/ arch flux-2-klein-4b; omitido = encoder oficial BFL. */
  textEncoderModelId?: string | null;
  prompt: string;
  negativePrompt?: string;
  width?: number;
  height?: number;
  steps?: number;
  guidanceScale?: number;
  seed?: number;
  quantization?: "none" | "2bit" | "4bit" | "6bit" | "8bit";
  sampler?: string;
  upscale?: { model: "4x" | "ultrasharp" | "siax"; scale: 2 | 4 } | null;
  distilled?: boolean;
  batchSize?: number;
  loras?: { modelId: string; scale: number }[];
  orchestratorId?: string | null;
  /* img2img (fatia feat/img2img — openapi 30140ea): XOR, no máximo um. */
  initImageId?: string | null;
  initGenerationId?: string | null;
  initStrength?: number;
}

/* Input efêmero p/ img2img — 201 de POST /api/generations/inputs. */
export interface GenerationInputUploaded {
  id: string;
  filename: string;
  mimeType: string;
  width: number;
  height: number;
}

export function diffusionGenerateErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros de geração de imagem inválidos.";
    case "unsupported_architecture":
      return "Arquitetura de checkpoint custom não suportada no v1 (somente SDXL e SD 1.5).";
    case "queue_unavailable":
      return "Fila de geração indisponível — tente novamente.";
    case "not_found":
      return "Pesos de modelo selecionados não encontrados.";
    default:
      return "Falha ao submeter job de geração de difusão.";
  }
}

export interface Generation {
  id: string;
  jobId: string | null;
  filename: string;
  url: string | null;
  thumbUrl: string | null;
  width: number;
  height: number;
  seed: number;
  prompt: string;
  negativePrompt?: string | null;
  params: Record<string, unknown>;
  createdAt: string;
}

export interface GenerationList {
  items: Generation[];
  total: number;
}

export interface GenerationIdsRequest {
  ids: string[];
}
