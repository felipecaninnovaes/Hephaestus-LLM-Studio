"use client";

import { useEffect, useMemo, useState } from "react";
import { Select, type SelectOption } from "@/components/ui/Select";
import { IconServer } from "@/components/icons";
import { listOrchestrators, type Orchestrator } from "@/lib/monitoring";

export interface NodeSelectProps {
  value: string | null;
  onChange: (nodeId: string | null) => void;
  disabled?: boolean;
  label?: string;
  hint?: string;
  size?: "sm" | "default" | "lg";
  className?: string;
}

/**
 * Seletor de nó de execução (ADR-0015 D2).
 *
 * Oferece "Automático (recomendado)" como primeira opção e lista os
 * orquestradores conhecidos do manager, indicando status (online/offline)
 * e recursos de aceleração/VRAM.
 */
export default function NodeSelect({
  value,
  onChange,
  disabled = false,
  label = "Nó de Execução",
  hint = "Selecione o ambiente alvo ou use Automático para roteamento por menor carga.",
  size = "default",
  className = "",
}: NodeSelectProps) {
  const [orchestrators, setOrchestrators] = useState<Orchestrator[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const res = await listOrchestrators();
        if (!cancelled) {
          setOrchestrators(res.items || []);
        }
      } catch {
        // Silenciosamente tolera indisponibilidade do manager
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, []);

  const options = useMemo<SelectOption<string>[]>(() => {
    const defaultOption: SelectOption<string> = {
      value: "",
      label: "Automático (recomendado)",
      description: "O escalonador despacha para o nó com maior capacidade disponível",
      badge: (
        <span className="rounded border border-brand-500/20 bg-brand-500/10 px-1.5 py-0.5 text-[10px] font-medium text-brand-400">
          Auto
        </span>
      ),
      icon: <IconServer className="w-4 h-4 text-brand-400" />,
    };

    const nodeOptions = orchestrators.map((node) => {
      const isOnline = node.status === "online";
      const hasGpu = (node.gpus?.length ?? 0) > 0;
      const vramLabel =
        node.vramTotal != null
          ? `${(node.vramTotal / 1024).toFixed(0)}GB VRAM`
          : hasGpu
          ? "GPU"
          : "CPU";

      return {
        value: node.id,
        label: `${node.name} (${node.kind})`,
        description: isOnline
          ? `${vramLabel} · ${node.jobsActive ?? 0} jobs ativos`
          : "Nó indisponível para execução",
        disabled: !isOnline,
        disabledReason: isOnline ? undefined : "Nó offline",
        badge: (
          <span
            className={`rounded border px-1.5 py-0.5 text-[10px] font-mono ${
              isOnline
                ? "border-[#34d399]/30 bg-[#34d399]/10 text-[#34d399]"
                : "border-zinc-700/50 bg-zinc-800/40 text-zinc-500"
            }`}
          >
            {isOnline ? "online" : "offline"}
          </span>
        ),
        icon: (
          <IconServer
            className={`w-4 h-4 ${
              isOnline ? "text-[#34d399]" : "text-zinc-500"
            }`}
          />
        ),
      };
    });

    return [defaultOption, ...nodeOptions];
  }, [orchestrators]);

  return (
    <div className={className}>
      <Select
        label={label}
        hint={hint}
        value={value ?? ""}
        onChange={(val) => onChange(val ? val : null)}
        options={options}
        disabled={disabled}
        loading={loading}
        size={size}
        placeholder="Selecione um nó..."
      />
    </div>
  );
}
