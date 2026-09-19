import { useMemo } from "react";
import type { Job, Telemetry } from "@/types/studio";

export interface SystemNotification {
  id: string;
  title: string;
  message: string;
  category: "infra" | "dataset" | "model" | "orchestrator";
  level: "info" | "warning" | "success" | "error";
  timestamp: string;
  actionLabel?: string;
  actionHref?: string;
}

export function useSystemNotifications(
  telemetry: Telemetry | null,
  jobs: Job[],
): SystemNotification[] {
  return useMemo<SystemNotification[]>(() => {
    const list: SystemNotification[] = [];

    // 1. Alertas e telemetria de Hardware
    if (telemetry) {
      const vramUsedMb = telemetry.vramUsed ?? 0;
      const vramTotalMb = telemetry.vramTotal ?? 0;
      const vramPct =
        vramTotalMb > 0 ? Math.round((vramUsedMb / vramTotalMb) * 100) : 0;

      if (vramPct >= 85) {
        list.push({
          id: "sys-vram-critical",
          title: "VRAM em Nível Crítico",
          message: `Consumo de GPU em ${vramPct}%. Risco iminente de CUDA OOM.`,
          category: "infra",
          level: "error",
          timestamp: "Tempo Real",
          actionLabel: "Ambientes",
          actionHref: "/environments",
        });
      } else if (vramPct >= 70) {
        list.push({
          id: "sys-vram-warn",
          title: "Atenção ao Uso de Memória",
          message: `Consumo de VRAM atingiu ${vramPct}%.`,
          category: "infra",
          level: "warning",
          timestamp: "Tempo Real",
          actionLabel: "Ver Nós",
          actionHref: "/environments",
        });
      }
    }

    // 2. Alertas de Jobs que falharam
    const failedJobs = jobs.filter((j) => j.status === "failed");
    failedJobs.slice(0, 2).forEach((job) => {
      list.push({
        id: `sys-job-failed-${job.id}`,
        title: `Falha no Processamento: ${job.model}`,
        message:
          job.error ||
          job.queueReason ||
          "O job foi encerrado de forma inesperada.",
        category: "orchestrator",
        level: "error",
        timestamp: job.finishedAt || job.createdAt,
        actionLabel: "Inspecionar",
        actionHref: `/jobs?job=${job.id}`,
      });
    });

    return list;
  }, [telemetry, jobs]);
}
