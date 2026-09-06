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

