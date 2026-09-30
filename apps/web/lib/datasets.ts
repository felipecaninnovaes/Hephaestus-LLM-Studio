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
  if (canAutoTrack(ds)) return "Executar AutoTracker neste dataset";
  if (ds.category !== "yolo") {
    return `AutoTracker disponível apenas para datasets YOLO (categoria atual: ${ds.category}).`;
  }
  if (ds.classes.length === 0) {
    return "AutoTracker exige dataset com ≥1 classe configurada.";
  }
  if (ds.imagesCount === 0) {
    return "AutoTracker exige dataset com ≥1 imagem.";
  }
  return "AutoTracker indisponível neste dataset.";
}

/** Habilita Treino de Difusão: dataset com ≥1 imagem. */
export function canTrainDiffusion(ds: Dataset): boolean {
  return ds.imagesCount > 0;
}

/** Motivo descritivo para title quando o treino de difusão estiver desabilitado. */
export function trainDiffusionDisabledReason(ds: Dataset): string {
  if (ds.imagesCount === 0) return "Treino de difusão exige dataset com ≥1 imagem.";
  return "Iniciar treino de difusão LoRA";
}

/** Habilita Treino unificado (suporta YOLO com ≥1 classe e ≥1 img OU Difusão com ≥1 img). */
export function canTrainDataset(ds: Dataset): boolean {
  if (ds.category === "yolo") {
    return ds.classes.length > 0 && ds.imagesCount > 0;
  }
  if (ds.category === "difusao" || (ds.category as string) === "diffusion") {
    return ds.imagesCount > 0;
  }
  return false;
}

/** Motivo descritivo para title quando o botão de treino estiver desabilitado (contextual por categoria). */
export function trainDatasetDisabledReason(ds: Dataset): string {
  if (ds.category === "yolo") {
    if (ds.classes.length === 0) return "Treino YOLO exige dataset com ≥1 classe.";
    if (ds.imagesCount === 0) return "Treino YOLO exige dataset com ≥1 imagem.";
    return "Abrir modal de treino YOLO";
  }
  if (ds.category === "difusao" || (ds.category as string) === "diffusion") {
    if (ds.imagesCount === 0) return "Treino de difusão exige dataset com ≥1 imagem.";
    return "Iniciar treino de difusão LoRA";
  }
  return "Treino disponível apenas para datasets YOLO e Difusão.";
}

/** Rótulo da ação de treino ("Treinar YOLO" vs "Treinar Difusão"). */
export function trainDatasetActionLabel(ds: Dataset): string {
  if (ds.category === "difusao" || (ds.category as string) === "diffusion") {
    return "Treinar Difusão";
  }
  return "Treinar YOLO";
}

