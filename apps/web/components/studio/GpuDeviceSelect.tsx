"use client";

import { useId, useMemo } from "react";
import { IconCpu } from "@/components/icons";
import { Select, type SelectOption } from "@/components/ui/Select";
import type { GpuDeviceTelemetry } from "@/lib/monitoring";

export interface GpuDeviceSelectProps {
	/** Nó selecionado (orchestratorId) ou null/undefined */
	orchestratorId?: string | null;
	/** Lista de gpuDevices do nó selecionado */
	devices?: GpuDeviceTelemetry[] | null;
	/** UUID da GPU selecionada ou null se Automático */
	value: string | null;
	/** Callback ao mudar seleção */
	onChange: (gpuDevice: string | null) => void;
	/** VRAM mínima exigida pelo fluxo/modelo em GB (opcional) */
	vramMinGb?: number | null;
	/** Desabilitar o seletor (ex: durante submit) */
	disabled?: boolean;
	label?: string;
	hint?: string;
	size?: "sm" | "default" | "lg";
	className?: string;
}

/**
 * Seletor de GPU individual para execução de jobs (Fatia F2 / ADR-0015 / multi-gpu-spec §2).
 *
 * - Só habilitado quando um nó com `gpuDevices` válidos estiver selecionado.
 * - Opção "Automático (menor que cabe)" como default (valor null).
 * - Uma opção por placa (índice, nome, VRAM livre/total em GB).
 * - Placas com VRAM total abaixo de `vramMinGb` ficam desabilitadas com motivo visível.
 */
export function GpuDeviceSelect({
	orchestratorId,
	devices,
	value,
	onChange,
	vramMinGb,
	disabled = false,
	label = "GPU de Execução",
	hint = "Automático seleciona a menor placa com VRAM suficiente para o modelo.",
	size = "default",
	className = "",
}: GpuDeviceSelectProps) {
	const selectId = useId();
	const hasGpus = Boolean(orchestratorId && devices && devices.length > 0);

	const options = useMemo<SelectOption<string>[]>(() => {
		const autoOption: SelectOption<string> = {
			value: "",
			label: "Automático (menor placa que cabe)",
			description: "O escalonador aloca a GPU com capacidade ideal para o job",
			badge: (
				<span className="rounded border border-brand-500/20 bg-brand-500/10 px-1.5 py-0.5 text-3xs font-medium text-brand-400">
					Auto
				</span>
			),
			icon: <IconCpu className="w-4 h-4 text-brand-400" />,
		};

		if (!hasGpus || !devices) {
			return [autoOption];
		}

		const deviceOptions: SelectOption<string>[] = devices.map((dev) => {
			const vramTotalGb = dev.vramTotal / 1024;
			const vramFreeGb = Math.max(0, (dev.vramTotal - dev.vramUsed) / 1024);
			const isInsufficient =
				vramMinGb != null && vramMinGb > 0 && vramTotalGb < vramMinGb;

			const disabledReason = isInsufficient
				? `Exige ≥${vramMinGb.toFixed(1)} GB VRAM (placa possui ${vramTotalGb.toFixed(1)} GB)`
				: undefined;

			return {
				value: dev.uuid,
				label: `GPU ${dev.index}: ${dev.name}`,
				description: `${vramFreeGb.toFixed(1)} GB livres · ${vramTotalGb.toFixed(1)} GB total`,
				disabled: isInsufficient,
				disabledReason,
				badge: (
					<span
						className={`rounded border px-1.5 py-0.5 text-3xs font-mono ${
							isInsufficient
								? "border-status-alert/30 bg-status-alert/10 text-amber-400"
								: "border-zinc-700/50 bg-zinc-800/40 text-zinc-300"
						}`}
					>
						{vramTotalGb.toFixed(0)} GB
					</span>
				),
				icon: (
					<IconCpu
						className={`w-4 h-4 ${
							isInsufficient ? "text-amber-400" : "text-brand-400"
						}`}
					/>
				),
			};
		});

		return [autoOption, ...deviceOptions];
	}, [hasGpus, devices, vramMinGb]);

	// Se o nó não tiver GPUs ou não houver nó selecionado, o componente fica desabilitado
	const isSelectDisabled = disabled || !hasGpus;
	const effectiveHint = !orchestratorId
		? "Selecione um nó específico para escolher uma GPU dedicada."
		: !hasGpus
			? "O nó selecionado opera em modo CPU/legado sem GPUs discretas dedicadas."
			: hint;

	return (
		<div className={className}>
			<Select
				id={selectId}
				label={label}
				hint={effectiveHint}
				value={value ?? ""}
				onChange={(val) => onChange(val ? val : null)}
				options={options}
				disabled={isSelectDisabled}
				size={size}
				placeholder="Selecione uma GPU..."
			/>
		</div>
	);
}

export default GpuDeviceSelect;
