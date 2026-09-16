import type { Job } from "@/types/studio";

export type MetricChipsFlavor = "yolo" | "diffusion" | "progress" | null;

export interface JobCapabilities {
  /** null = sem chips; "progress" = chip único de progresso derivado do job */
  metricChips: MetricChipsFlavor;
  /** Gráfico de convergência (ConvergenceChart) */
  convergenceChart: boolean;
  /** Renderiza se houver artefatos de imagem (o componente já retorna null sem png) */
  samplesGallery: boolean;
  /** Botões "Revisar e Aplicar" */
  applyAction: boolean;
  /** "Repetir Treino" / clone */
  rerun: boolean;
}

/** Mapa canônico de capacidades por kind/engine — única fonte de verdade (AC-002). */
export function jobCapabilities(job: Pick<Job, "kind" | "engine">): JobCapabilities {
  const kind = String(job.kind);

  // diffusion_train é o alias legado de "diffusion"; engine "diffusion" também mapeia aqui
  const isDiffusion =
    kind === "diffusion" ||
    kind === "diffusion_train" ||
    job.engine === "diffusion";

  const isYoloTrain = kind === "yolo_train";
  const isAutolabel = kind === "autolabel";
  const isAutotracker = kind === "autotracker";
  const isYoloPredict = kind === "yolo_predict";
  const isDiffusionGenerate = kind === "diffusion_generate";

  // Default
  const caps: JobCapabilities = {
    metricChips: null,
    convergenceChart: false,
    samplesGallery: true,
    applyAction: false,
    rerun: true,
  };

  if (isYoloTrain) {
    caps.metricChips = "yolo";
    caps.convergenceChart = true;
    caps.samplesGallery = false;
  } else if (isDiffusion) {
    caps.metricChips = "diffusion";
    caps.convergenceChart = true;
    caps.samplesGallery = true;
  } else if (isAutolabel) {
    caps.metricChips = "progress";
    caps.convergenceChart = false;
    caps.samplesGallery = false;
    caps.applyAction = true;
  } else if (isAutotracker) {
    caps.metricChips = "progress";
    caps.convergenceChart = false;
    caps.samplesGallery = true;
    caps.applyAction = true;
  } else if (isYoloPredict) {
    caps.samplesGallery = true;
    caps.rerun = false;
  } else if (isDiffusionGenerate) {
    caps.samplesGallery = true;
    caps.rerun = false;
  }
  // default: já setado acima

  return caps;
}

/** Rótulo do chip de progresso de imagens: `step · %` quando ambos, um só quando parcial, "—" quando nada. */
export function imageProgressLabel(step: number | null | undefined, progress: number | null | undefined): string {
  const pct = Number.isFinite(progress) ? `${Math.round((progress as number) * 100)}%` : null;
  if (step != null && pct) return `${step} · ${pct}`;
  if (step != null) return String(step);
  if (pct) return pct;
  return "—";
}
