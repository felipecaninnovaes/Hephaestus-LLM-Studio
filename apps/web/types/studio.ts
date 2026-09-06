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

