import type { ElementType } from "react";
import { IconLayers, IconTarget, IconZap } from "@/components/icons";
import type { Job } from "@/types/jobs";

export interface JobServiceInfo {
  serviceTitle: string;
  categoryLabel: string;
  icon: ElementType;
  actionText: string;
  targetHref: string;
}

export function getJobServiceInfo(job: Job): JobServiceInfo {
  if (job.kind === "yolo_train") {
    return {
      serviceTitle: "Treino YOLO",
      categoryLabel: "Visão Computacional",
      icon: IconTarget,
      actionText: "Ver na Forja →",
      targetHref: `/jobs?job=${job.id}`,
    };
  }
  if (job.kind === "autotracker") {
    return {
      serviceTitle: "AutoTracker",
      categoryLabel: "Rastreamento & Vídeo",
      icon: IconLayers,
      actionText: job.datasetId ? "Ver no Dataset →" : "Ver no Studio →",
      targetHref: job.datasetId
        ? `/datasets/${job.datasetId}`
        : `/jobs?job=${job.id}`,
    };
  }
  const kindName = String(job.kind).replace(/_/g, " ");
  return {
    serviceTitle: kindName.charAt(0).toUpperCase() + kindName.slice(1),
    categoryLabel: "Processamento IA",
    icon: IconZap,
    actionText: "Ver Detalhes →",
    targetHref: `/jobs?job=${job.id}`,
  };
}
