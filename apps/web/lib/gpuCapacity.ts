import type { GpuDeviceTelemetry, Orchestrator } from "@/lib/monitoring";

export interface TargetGpu {
	vramTotalGb: number;
	label: string;
}

/** MiB → GB com 1 casa decimal (12288 → 12). */
function mibToGb(mib: number): number {
	return Math.round((mib / 1024) * 10) / 10;
}

function deviceTarget(dev: GpuDeviceTelemetry): TargetGpu {
	const vramTotalGb = mibToGb(dev.vramTotal);
	return { vramTotalGb, label: `GPU ${dev.index}: ${dev.name} (${vramTotalGb} GB)` };
}

function largestDevice(
	devices: GpuDeviceTelemetry[] | null | undefined,
): GpuDeviceTelemetry | null {
	let best: GpuDeviceTelemetry | null = null;
	for (const dev of devices ?? []) {
		if (dev.vramTotal > 0 && (!best || dev.vramTotal > best.vramTotal)) best = dev;
	}
	return best;
}

/** Maior placa do nó; sem `gpuDevices`, cai para o `vramTotal` (MiB) agregado do nó. */
function nodeTarget(node: Orchestrator): TargetGpu | null {
	const dev = largestDevice(node.gpuDevices);
	if (dev) return deviceTarget(dev);
	if (node.vramTotal != null && node.vramTotal > 0) {
		const vramTotalGb = mibToGb(node.vramTotal);
		return { vramTotalGb, label: `${node.name} (${vramTotalGb} GB)` };
	}
	return null;
}

/**
 * GPU em que o job vai rodar, para risco de OOM e rótulo do dispositivo.
 * Um job usa uma única placa: GPU escolhida → maior placa do nó escolhido →
 * (Automático) maior placa entre os nós online. `/api/telemetry` é a soma de
 * todas as GPUs e não representa capacidade de um job. `null` = nada medível.
 */
export function resolveTargetGpu(
	orchestrators: Orchestrator[],
	selectedOrchestratorId: string | null,
	selectedGpuDevice: string | null,
): TargetGpu | null {
	if (selectedOrchestratorId) {
		const node = orchestrators.find((o) => o.id === selectedOrchestratorId);
		if (!node) return null;
		const chosen = selectedGpuDevice
			? node.gpuDevices?.find((d) => d.uuid === selectedGpuDevice)
			: undefined;
		return chosen ? deviceTarget(chosen) : nodeTarget(node);
	}

	let best: TargetGpu | null = null;
	for (const node of orchestrators) {
		if (node.status !== "online") continue;
		const target = nodeTarget(node);
		if (target && (!best || target.vramTotalGb > best.vramTotalGb)) best = target;
	}
	return best;
}
