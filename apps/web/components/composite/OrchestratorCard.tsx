"use client";

import type { ReactNode } from "react";
import { GlassCard } from "@/components/ui/GlassCard";
import { MetricTile } from "@/components/ui/MetricTile";
import { TruncatedText } from "@/components/ui/TruncatedText";
import { formatRelativeTime } from "@/lib/format";
import {
  STATUS_CLASSES,
  STATUS_LABELS,
  nodeMetrics,
  type Orchestrator,
} from "@/lib/monitoring";

const fmt = (
  v: number | null | undefined,
  decimals = 1,
  suffix = "",
): string => (v != null ? `${v.toFixed(decimals)}${suffix}` : "—");

export interface OrchestratorCardProps {
  node: Orchestrator;
  action?: ReactNode;
  className?: string;
}

/**
 * Card canônico de nó orquestrador (GPU/CPU local ou remoto).
 * Exibe telemetria em tempo real, capacidade de VRAM e medidores de recursos.
 */
export function OrchestratorCard({
  node,
  action,
  className = "",
}: OrchestratorCardProps) {
  const m = nodeMetrics(node);

  return (
    <GlassCard className={`p-0 overflow-hidden ${className}`.trim()}>
      <div className="space-y-4 p-5 sm:p-6">
        {/* Header do Card */}
        <div className="flex flex-col gap-3 border-b border-white/10 pb-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0 space-y-1.5">
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <div className="max-w-full min-w-0 text-lg font-semibold tracking-tight text-white break-words">
                <TruncatedText text={node.name} as="span" />
              </div>

              {/* Badge Kind */}
              <span className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-brand-300 bg-brand-500/15 border-brand-500/30 backdrop-blur-sm px-2 py-0.5 text-2xs">
                {node.kind === "local" ? "Local" : "Remoto"}
              </span>

              {/* Status badge */}
              <div
                className={`inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium backdrop-blur-sm px-2 py-0.5 text-2xs font-mono ${
                  STATUS_CLASSES[node.status] ?? STATUS_CLASSES.unknown
                }`}
              >
                <span>{STATUS_LABELS[node.status] ?? node.status}</span>
                {(node.status === "online" || node.status === "degraded") && (
                  <span className="relative ml-1.5 flex h-2 w-2">
                    <span
                      className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-75 motion-reduce:animate-none ${
                        node.status === "online"
                          ? "bg-status-success"
                          : "bg-status-alert"
                      }`}
                    />
                    <span
                      className={`relative inline-flex h-2 w-2 rounded-full ${
                        node.status === "online"
                          ? "bg-status-success"
                          : "bg-status-alert"
                      }`}
                    />
                  </span>
                )}
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-2xs text-zinc-400">
              <TruncatedText
                text={node.endpoint}
                className="font-mono"
                as="span"
              />
              {node.lastHeartbeat && (
                <>
                  <span>·</span>
                  <span className="font-mono">
                    visto há {formatRelativeTime(node.lastHeartbeat)}
                  </span>
                </>
              )}
            </div>
          </div>

          {/* Slot de ação no topo direito (ex.: botões de adotar/revogar) */}
          {action && (
            <div className="flex shrink-0 items-center gap-2">{action}</div>
          )}
        </div>

        {/* Status derivado de dados reais */}
        <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm text-zinc-400">
          <span className="font-medium text-zinc-200">
            {node.jobsActive > 0
              ? `${node.jobsActive} treino ativo`
              : "Idle (pronto)"}
          </span>
          {node.vramTotalGb != null && (
            <>
              <span className="text-zinc-500">·</span>
              <span className="font-medium text-zinc-200">
                capacidade {node.vramTotalGb} GB
              </span>
            </>
          )}
        </div>

        {/* Medidores de Recursos — por nó */}
        {node.measured ? (
          <div className="border-t border-white/10 pt-4">
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              {/* GPU */}
              <MetricTile
                label="USO DA GPU"
                value={m.gpuLabel ?? "sem GPU"}
                highlightColor={m.gpuLabel ? "brand" : "default"}
                subtext={m.hasGpu ? "nome da placa" : "sem GPU instalada"}
              />

              {/* VRAM */}
              <MetricTile
                label="USO DA VRAM"
                value={
                  m.vramUsedGb && m.vramTotalGb
                    ? `${m.vramUsedGb} / ${m.vramTotalGb} GB`
                    : "—"
                }
                highlightColor="cyan"
                subtext={fmt(m.vramPct, 1, "%")}
              />

              {/* Sistema & Host */}
              <MetricTile
                label="SISTEMA & HOST"
                value={fmt(m.cpuPct, 1, "%")}
                highlightColor="default"
                subtext={
                  m.ramUsedGb && m.ramTotalGb
                    ? `${m.ramUsedGb} / ${m.ramTotalGb} GB RAM`
                    : "RAM: —"
                }
              />
            </div>

            {/* GPUs detalhadas */}
            {m.hasGpu && node.gpus && node.gpus.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {node.gpus.map((gpu) => (
                  <span
                    key={gpu}
                    className="rounded-full border border-white/10 bg-black/40 px-2.5 py-0.5 font-mono text-2xs text-zinc-300"
                  >
                    {gpu}
                  </span>
                ))}
              </div>
            )}
          </div>
        ) : (
          <div className="border-t border-white/10 pt-4 text-xs text-zinc-500">
            Telemetria indisponível (heartbeat ausente).
          </div>
        )}
      </div>
    </GlassCard>
  );
}
