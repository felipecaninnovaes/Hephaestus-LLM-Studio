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
  boxesCount?: number | null;
  caption?: string | null;
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

export type JobKind = "yolo_train" | "autotracker" | "yolo_predict" | "autolabel" | "diffusion" | "diffusion_train" | "diffusion_generate";

export interface JobMetrics {
  epoch: number;
  boxLoss?: number;
  clsLoss?: number;
  dflLoss?: number;
  map50?: number;
  map5095?: number;
  loss?: number;
  lr?: number;
  step?: number;
  progress?: number;
  phase?: string;
  message?: string;
}

export interface JobMetricsResponse {
  items: JobMetrics[];
}

export interface JobTelemetryEvent {
  timestamp: string;
  phase: string;
  phaseMessage?: string | null;
  progress: number;
  step?: number | null;
  totalSteps?: number | null;
  epoch?: number | null;
  totalEpochs?: number | null;
  vramUsedGb?: number | null;
  metrics?: Record<string, number | string | boolean | null> | null;
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
  error?: string | null;
  phase?: string | null;
  phaseMessage?: string | null;
  vramUsedGb?: number | null;
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

export interface BatchBoxesUpdateRequest {
  imageIds?: string[] | null;
  action: "remap" | "delete";
  sourceClassId: string;
  targetClassId?: string | null;
}

export interface BatchBoxesUpdateResponse {
  affectedBoxes: number;
  affectedImages: number;
}

/* ── AutoLabel (ADR-0016 / ADR-0019 AutoLabel v2) ─────────── */

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

/* ── Difusão LoRA (ADR-0018) ─────────────────────────────────── */

export interface DiffusionJobRequest {
  datasetId: string;
  /** "flux" (FLUX.2 Klein 4B), "sdxl" (SDXL 1.0) ou "sd15" (Stable Diffusion 1.5) */
  baseModel: "sdxl" | "flux" | "sd15";
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
  optimizer?: "adamw8bit" | "adamw" | "prodigy";
  lrScheduler?: "cosine" | "linear" | "constant" | "constant_with_warmup";
  lrWarmupSteps?: number;
  mixedPrecision?: "fp16" | "bf16" | "no";
  quantization?: "none" | "4bit" | "8bit";
  outputName?: string | null;
}

export interface DiffusionPreset {
  name: string;
  description?: string;
  version?: string;
  baseModel: "sdxl" | "flux" | "sd15";
  triggerWord?: string;
  epochs: number;
  batchSize: number;
  learningRate: string;
  rank: number;
  alpha: number;
  resolution?: number;
  gradientAccumulationSteps?: number;
  optimizer?: "adamw8bit" | "adamw" | "prodigy";
  lrScheduler?: "cosine" | "linear" | "constant" | "constant_with_warmup";
  lrWarmupSteps?: number;
  mixedPrecision?: "fp16" | "bf16" | "no";
  quantization?: "none" | "4bit" | "8bit";
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
  baseModel: "sdxl" | "flux" | "sd15" | "flux-2-klein-4b";
  prompt: string;
  negativePrompt?: string;
  width?: number;
  height?: number;
  steps?: number;
  guidanceScale?: number;
  seed?: number;
  quantization?: "none" | "4bit" | "8bit";
  distilled?: boolean;
  weights?: string | null;
  loraScale?: number;
  orchestratorId?: string | null;
}

export function diffusionGenerateErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros de geração de imagem inválidos.";
    case "queue_unavailable":
      return "Fila de geração indisponível — tente novamente.";
    case "not_found":
      return "Pesos de modelo LoRA selecionados não encontrados.";
    default:
      return "Falha ao submeter job de geração de difusão.";
  }
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

