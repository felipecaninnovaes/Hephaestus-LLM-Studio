export interface YoloAugment {
  mosaic: boolean;
  mixupFlip: boolean;
}

export interface AutotrackerJobRequest {
  datasetId: string;
  model?: string;
  conf?: number;
  modelId?: string;
  orchestratorId?: string | null;
}

export interface AutotrackerApplyRequest {
  overwrite?: boolean;
  imageId?: string | null;
  createMissingClasses?: string[] | null;
}

export interface AutotrackerClassCount {
  name: string;
  boxesCount: number;
}

export interface AutotrackerPreviewResponse {
  totalImages: number;
  totalBoxes: number;
  existingClasses: AutotrackerClassCount[];
  missingClasses: AutotrackerClassCount[];
}

export interface AutotrackerApplyResponse {
  applied: number;
  skipped: number;
  images: number;
}

export type AutolabelModel = "mock" | "florence-2" | "qwen2-vl" | "openai";

export interface AutolabelJobRequest {
  datasetId: string;
  model?: AutolabelModel | string;
  prompt?: string;
  apiKey?: string;
  apiBase?: string;
  openaiModel?: string;
  orchestratorId?: string | null;
  reasoningEffort?: "none" | "low" | "medium" | "high" | null;
  filterClassId?: string | null;
  imageIds?: string[] | null;
}

export interface AutolabelApplyItem {
  filename: string;
  caption: string;
}

export interface AutolabelApplyRequest {
  datasetId?: string;
  overwrite?: boolean;
  items?: AutolabelApplyItem[];
}

export interface AutolabelPreviewItem {
  imageId: string;
  filename: string;
  imageUrl: string;
  generatedCaption: string;
  currentCaption?: string | null;
  currentOrigin?: string | null;
}

export interface AutolabelPreviewResponse {
  jobId: string;
  datasetId: string;
  model?: string | null;
  totalGenerated: number;
  items: AutolabelPreviewItem[];
}

export interface AutolabelApplyResponse {
  applied: number;
  skipped: number;
  images: number;
}

export function autolabelErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros do AutoLabel inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto — exige ≥1 imagem.";
    case "queue_unavailable":
      return "Fila de processamento indisponível — tente novamente.";
    case "not_found":
      return "Job não encontrado.";
    case "job_not_done":
      return "O job ainda não terminou — aguarde a conclusão.";
    case "storage_unavailable":
      return "Armazenamento de artefatos indisponível — tente novamente.";
    default:
      return "Falha ao processar AutoLabel.";
  }
}

export function autotrackerErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros do AutoTracker inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto — exige category yolo, ≥1 classe e ≥1 imagem.";
    case "queue_unavailable":
      return "Fila de processamento indisponível — tente novamente.";
    case "not_found":
      return "Job não encontrado.";
    case "job_not_done":
      return "O job ainda não terminou — aguarde a conclusão.";
    case "storage_unavailable":
      return "Armazenamento de artefatos indisponível — tente novamente.";
    default:
      return "Falha ao processar AutoTracker.";
  }
}

export interface PredictJobRequest {
  modelId: string;
  datasetId: string;
  conf?: number;
  orchestratorId?: string | null;
}

/** Coordenadas normalizadas 0..1 do predictions.json (snake_case). */
export interface PredictionBox {
  class: string;
  x: number;
  y: number;
  w: number;
  h: number;
  conf: number;
}

export interface PredictionImage {
  filename: string;
  boxes: PredictionBox[];
}

export interface PredictionsData {
  engine: string;
  model: string;
  conf: number;
  images: PredictionImage[];
}

export function predictErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros de inferência inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto — exige category yolo e ≥1 imagem.";
    case "queue_unavailable":
      return "Fila de processamento indisponível — tente novamente.";
    case "not_found":
      return "Modelo ou dataset não encontrado.";
    default:
      return "Falha ao iniciar inferência.";
  }
}
