export type DatasetCategory = "difusao" | "openclip" | "yolo";
export type DatasetType =
  | "yolo_bbox"
  | "yolo_seg"
  | "difusao_lora"
  | "clip_image_text";
export type DatasetTask = "detect_track" | "segment" | "caption" | "embedding";
export type DatasetFormat = "yolo_txt" | "captions" | "pairs";
export type DatasetStatus = "ready" | "in_progress" | "needs_labeling";

export interface StudioClass {
  id: string;
  name: string;
  idx: number;
  color: string;
}

export interface Dataset {
  id: string;
  slug: string;
  title: string;
  category: DatasetCategory;
  type: DatasetType;
  task: DatasetTask;
  format: DatasetFormat;
  status: DatasetStatus;
  source: string | null;
  sizeBytes: number;
  imagesCount: number;
  labeledCount: number;
  classes: StudioClass[];
  autoTracked: boolean;
  trashCount: number;
  createdAt: string;
  lastModified: string;
}

export interface CreateDatasetRequest {
  title: string;
  type: DatasetType;
  classes?: string[];
}

export const TYPE_LABELS: Record<DatasetType, string> = {
  yolo_bbox: "YOLO · Detecção",
  yolo_seg: "YOLO · Segmentação",
  difusao_lora: "Difusão · LoRA",
  clip_image_text: "OpenCLIP · Embedding",
};

export const CATEGORY_LABELS: Record<DatasetCategory, string> = {
  difusao: "Difusão",
  openclip: "OpenCLIP",
  yolo: "YOLO",
};

export const STATUS_LABELS: Record<DatasetStatus, string> = {
  ready: "Pronto",
  in_progress: "Em andamento",
  needs_labeling: "Aguardando rotulagem",
};

export interface ImageItem {
  id: string;
  filename: string;
  objectKey: string;
  bytes: number;
  width: number;
  height: number;
  mediaType: string;
  split: string;
  url: string;
  createdAt: string;
}

export interface ImagePage {
  items: ImageItem[];
  total: number;
  limit: number;
  offset: number;
}

export interface BBoxData {
  id: string;
  classId: string;
  x: number;
  y: number;
  w: number;
  h: number;
  conf: number | null;
  origin: string;
  trackId: number | null;
}

export interface CaptionData {
  text: string;
  origin: string;
  model: string | null;
  updatedAt: string;
}

export interface ImageDetail {
  id: string;
  datasetId?: string;
  filename: string;
  objectKey: string;
  bytes: number;
  width: number;
  height: number;
  mediaType: string;
  split: string;
  url: string;
  createdAt: string;
  boxes: BBoxData[];
  caption: CaptionData | null;
}

export type UploadResultStatus = "stored" | "duplicate" | "rejected" | "failed";

export type UploadResultReason =
  | "duplicate_filename"
  | "unsupported_media"
  | "too_large"
  | "storage_error"
  | "envelope_limit"
  | null;

export interface UploadResultItem {
  imageId: string | null;
  filename: string;
  status: UploadResultStatus;
  reason: UploadResultReason;
  bytes: number | null;
  width: number | null;
  height: number | null;
}

export interface BoxInput {
  classId: string;
  x: number;
  y: number;
  w: number;
  h: number;
  conf?: number | null;
  origin?: string;
  trackId?: number | null;
}

export interface PutBoxesResponse {
  boxes: BBoxData[];
}

export interface PutClassInput {
  id?: string;
  name: string;
}

export interface PutClassesResponse {
  classes: StudioClass[];
}

export type SearchIndexStatus = "not_indexed" | "indexing" | "ready" | "stale";

export interface SearchStatus {
  status: SearchIndexStatus;
  imagesCount: number;
  indexedCount: number;
  model: string;
  dim: number;
}

export interface SearchItem {
  image: ImageItem;
  score: number;
}

export interface SearchResponse {
  items: SearchItem[];
}

/* ── Jobs (F4.7) ──────────────────────────────────────────── */

export type JobStatus =
  | "queued"
  | "running"
  | "cancelling"
  | "done"
  | "failed"
  | "cancelled";

export type JobKind = "yolo_train" | "autotracker" | "yolo_predict";

export interface JobMetrics {
  epoch: number;
  boxLoss: number;
  clsLoss: number;
  dflLoss: number;
  map50: number;
  map5095: number;
}

export interface JobMetricsResponse {
  items: JobMetrics[];
}

export interface Job {
  id: string;
  kind: JobKind;
  engine: string;
  model: string;
  mode: string | null;
  datasetId: string;
  status: JobStatus;
  queueReason: string | null;
  queuePosition: number | null;
  progress: number;
  epoch: number | null;
  step: number | null;
  metrics: Record<string, unknown> | null;
  vramMinGb: number | null;
  orchestratorId: string | null;
  orchestratorName?: string | null;
  orchestratorKind?: "docker" | "slurm" | "local" | "remoto" | null;
  orchestratorFallback: boolean;
  createdAt: string;
  finishedAt: string | null;
}

export interface JobListResponse {
  items: Job[];
  total: number;
}

export interface JobArtifact {
  id: string;
  kind: string;
  path: string;
  md5: string;
  bytes: number;
}

export interface JobArtifactsResponse {
  items: JobArtifact[];
}

export interface Telemetry {
  measured: boolean;
  cpu: number | null;
  ram: number | null;
  ramTotal: number | null;
  vramUsed: number | null;
  vramTotal: number | null;
  gpus: string[];
  jobsActive: number;
}

/* ── Augment toggles for YOLO train ──────────────────────── */
export interface YoloAugment {
  mosaic: boolean;
  mixupFlip: boolean;
}

/* ── AutoTracker (Fatia 5 — ADR-0008) ─────────────────────── */

export interface AutotrackerJobRequest {
  datasetId: string;
  model?: string;
  conf?: number;
  modelId?: string;
  orchestratorId?: string | null;
}

export interface AutotrackerApplyResponse {
  applied: number;
  skipped: number;
  images: number;
}

/* ── Toast helpers for jobs ────────────────────────────────── */
/* ── Models (Fatia I — ADR-0012 D6/D7) ──────────────────────── */

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
}

export interface ModelListResponse {
  items: Model[];
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

export function modelErrorMessage(code: string): string {
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

export type JobErrorCode =
  | "invalid_request"
  | "dataset_not_ready"
  | "queue_unavailable"
  | "not_found"
  | "job_not_done"
  | "storage_unavailable";

export function jobErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros de treino inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto para treino.";
    case "queue_unavailable":
      return "Fila de treino indisponível — tente novamente.";
    case "not_found":
      return "Job não encontrado.";
    default:
      return "Falha ao criar job de treino.";
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

/* ── Playground / Inferência YOLO (Fatia J — ADR-0013 D7) ───── */

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

