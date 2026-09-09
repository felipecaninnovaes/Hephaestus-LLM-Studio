import { apiFetch } from "@/lib/api";
import type { CreateDatasetRequest, Dataset } from "@/types/studio";

export function listDatasets(): Promise<Dataset[]> {
  return apiFetch<Dataset[]>("/api/datasets");
}

export function createDataset(req: CreateDatasetRequest): Promise<Dataset> {
  return apiFetch<Dataset>("/api/datasets", {
    method: "POST",
    body: req,
  });
}

export function getDataset(id: string): Promise<Dataset> {
  return apiFetch<Dataset>(`/api/datasets/${id}`);
}

export function deleteDataset(id: string): Promise<void> {
  return apiFetch<void>(`/api/datasets/${id}`, { method: "DELETE" });
}

/** Habilita Treino YOLO: category yolo + ≥1 classe + ≥1 imagem. */
export function canTrainYolo(ds: Dataset): boolean {
  return ds.category === "yolo" && ds.classes.length > 0 && ds.imagesCount > 0;
}

/** Motivo descritivo para title quando o botão de treino estiver desabilitado. */
export function trainDisabledReason(ds: Dataset): string {
  if (ds.category !== "yolo") return "Treino disponível apenas para datasets YOLO.";
  if (ds.classes.length === 0) return "Treino YOLO exige dataset yolo com ≥1 classe.";
  if (ds.imagesCount === 0) return "Treino YOLO exige dataset yolo com ≥1 imagem.";
  return "Abrir modal de treino YOLO";
}

/** Habilita AutoTracker: category yolo + ≥1 classe + ≥1 imagem. */
export function canAutoTrack(ds: Dataset): boolean {
  return ds.category === "yolo" && ds.classes.length > 0 && ds.imagesCount > 0;
}

/** Motivo descritivo para title do AutoTracker. */
export function autoTrackDisabledReason(ds: Dataset): string {
  if (!canAutoTrack(ds)) {
    return "AutoTracker exige dataset yolo com ≥1 classe e ≥1 imagem";
  }
  return "Executar AutoTracker neste dataset";
}

