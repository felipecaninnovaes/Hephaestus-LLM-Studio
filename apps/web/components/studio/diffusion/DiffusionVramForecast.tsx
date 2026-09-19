"use client";

import { IconAlertTriangle, IconCpu, IconZap } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import type { OomRisk } from "@/hooks/useVramEstimator";

export interface DiffusionVramForecastProps {
  estimatedVram: number;
  nodeVramTotalGb: number | null;
  deviceLabel: string;
  oomRisk: OomRisk;
  onAutoFixSafeParams: () => void;
}

export function DiffusionVramForecast({
  estimatedVram,
  nodeVramTotalGb,
  deviceLabel,
  oomRisk,
  onAutoFixSafeParams,
}: DiffusionVramForecastProps) {
  return (
    <div
      className={`rounded-xl border p-3.5 space-y-2.5 transition backdrop-blur-sm ${
        oomRisk === "danger"
          ? "border-rose-500/40 bg-rose-500/[0.06]"
          : oomRisk === "warning"
            ? "border-status-alert/35 bg-status-alert/[0.05]"
            : "border-white/10 bg-white/[0.02]"
      }`}
    >
      <div className="flex items-center justify-between font-mono text-2xs">
        <span className="tracking-caps font-medium uppercase text-zinc-400 flex items-center gap-1.5">
          <IconZap className="size-3.5 text-brand-400" />
          VRAM Estimada para Treino
        </span>
        <span
          className={`font-semibold ${
            oomRisk === "danger"
              ? "text-rose-400"
              : oomRisk === "warning"
                ? "text-amber-300"
                : "text-zinc-100"
          }`}
        >
          ~{estimatedVram} GB {nodeVramTotalGb ? `/ ${nodeVramTotalGb} GB` : ""}
        </span>
      </div>

      {/* Barra de Consumo de VRAM */}
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800/80 border border-white/5">
        <div
          className={`h-full rounded-full transition-all duration-300 motion-reduce:transition-none ${
            oomRisk === "danger"
              ? "bg-rose-500"
              : oomRisk === "warning"
                ? "bg-amber-400"
                : "bg-brand-500"
          }`}
          style={{
            width: `${Math.min(
              100,
              Math.max(
                6,
                Math.round((estimatedVram / (nodeVramTotalGb || 24)) * 100),
              ),
            )}%`,
          }}
        />
      </div>

      {/* Dispositivo de Destino */}
      <div className="flex items-center justify-between font-mono text-2xs text-zinc-400">
        <span className="flex items-center gap-1">
          <IconCpu className="size-3" /> Dispositivo:
        </span>
        <span
          className="text-zinc-300 truncate max-w-[200px]"
          title={deviceLabel}
        >
          {deviceLabel}
        </span>
      </div>

      {/* Alerta Preventivo de CUDA OOM */}
      {oomRisk !== "safe" && (
        <div
          className={`rounded-lg border p-2.5 space-y-2 ${
            oomRisk === "danger"
              ? "border-rose-500/30 bg-rose-950/40 text-rose-200"
              : "border-status-alert/30 bg-amber-950/40 text-amber-200"
          }`}
        >
          <div className="flex items-start gap-2">
            <IconAlertTriangle
              className={`size-4 shrink-0 mt-0.5 ${
                oomRisk === "danger" ? "text-rose-400" : "text-amber-400"
              }`}
            />
            <div className="space-y-1 font-mono text-2xs">
              <p className="font-semibold text-white">
                {oomRisk === "danger"
                  ? "Risco Crítico de Memória de Vídeo (CUDA OOM)"
                  : "Atenção: Uso de VRAM Elevado"}
              </p>
              <p className="text-zinc-300 leading-snug">
                {oomRisk === "danger"
                  ? `A configuração selecionada requer ~${estimatedVram} GB de VRAM${
                      nodeVramTotalGb
                        ? ` (capacidade do nó: ${nodeVramTotalGb} GB)`
                        : ""
                    }. O job provavelmente falhará por falta de memória.`
                  : `A estimativa de ~${estimatedVram} GB opera próxima ao limite máximo de alocação da GPU.`}
              </p>
            </div>
          </div>

          {/* Ação de Auto-Fix */}
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onAutoFixSafeParams}
            className="w-full font-mono text-2xs"
          >
            Ajustar para Perfil Leve (SD 1.5 · 512px · Batch 1 · GA 2x)
          </Button>
        </div>
      )}
    </div>
  );
}
