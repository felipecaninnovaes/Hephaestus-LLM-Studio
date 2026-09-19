export type ModelSource = "train" | "upload" | "download";

export interface Model {
  id: string;
  name: string;
  engine: string;
  model: string | null;
  source: ModelSource;
  bytes: number;
  md5: string;
  url: string | null;
  jobId: string | null;
  createdAt: string;
  kind?: "lora" | "checkpoint" | "text_encoder" | null;
  arch?: "flux-2-klein-4b" | "sdxl" | "sd15" | null;
}

export interface ModelListResponse {
  items: Model[];
}

export interface ModelUploadInitRequest {
  name: string;
  engine: string;
  kind?: string;
  arch?: string;
  size: number;
  totalParts: number;
}

export interface ModelUploadInitResponse {
  uploadId: string;
  partSize: number;
  totalParts: number;
}

export function modelSourceLabel(source: ModelSource): string {
  switch (source) {
    case "train":
      return "Treino";
    case "upload":
      return "Upload";
    case "download":
      return "Download";
  }
}

export function modelErrorMessage(code: string, message?: string): string {
  if (code === "invalid_request" && message) {
    const lower = message.toLowerCase();
    if (lower.includes("sniff and hint conflict")) {
      return "Conflito entre detecção automática e hint manual. Verifique os seletores de kind/arch e tente novamente.";
    }
    if (lower.includes("kind/arch could not be determined")) {
      return "Não foi possível detectar kind/arch automaticamente. Use os seletores manuais (kind + arch) e tente novamente.";
    }
    if (lower.includes("invalid kind or arch")) {
      return "Valores de kind ou arch inválidos. Verifique os seletores e tente novamente.";
    }
    if (lower.includes("invalid safetensors header")) {
      return "Cabeçalho do safetensors inválido ou corrompido. Verifique o arquivo.";
    }
  }

  switch (code) {
    case "invalid_request":
      return "Parâmetros inválidos.";
    case "queue_unavailable":
      return "Fila de processamento indisponível — tente novamente.";
    case "not_found":
      return "Modelo não encontrado.";
    case "model_download_disabled":
      return "Download por URL desabilitado — configure MODEL_DOWNLOAD_ALLOWED_HOSTS no ambiente.";
    case "model_download_failed":
      return "Falha ao baixar o modelo por URL — verifique o endereço e tente novamente.";
    case "storage_unavailable":
      return "Armazenamento indisponível — tente novamente.";
    default:
      return "Falha na operação de modelo.";
  }
}

export interface LoraRef {
  modelId: string;
  scale: number;
}
