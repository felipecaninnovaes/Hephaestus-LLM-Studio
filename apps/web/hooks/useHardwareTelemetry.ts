"use client";

import { useMemo } from "react";
import { useSharedTelemetry } from "@/lib/telemetryStore";
import type { Telemetry } from "@/types/studio";

export interface HardwareTelemetryState {
	telemetry: Telemetry | null;
	nodeVramTotalGb: number | null;
	deviceLabel: string;
}

/**
 * Hook de telemetria de hardware com polling compartilhado e consciente de visibilidade da página.
 * Fornece estado de hardware, capacidade de VRAM em GB e rótulo do dispositivo.
 */
export function useHardwareTelemetry(
	pollingInterval = 10000,
): HardwareTelemetryState {
	const telemetry = useSharedTelemetry(pollingInterval);

	const nodeVramTotalGb = useMemo(() => {
		if (telemetry?.vramTotal && telemetry.vramTotal > 0) {
			return telemetry.vramTotal > 1000
				? Math.round((telemetry.vramTotal / (1024 * 1024 * 1024)) * 10) / 10
				: telemetry.vramTotal;
		}
		return null;
	}, [telemetry?.vramTotal]);

	const deviceLabel = useMemo(() => {
		if (telemetry?.gpus && telemetry.gpus.length > 0) {
			return `${telemetry.gpus[0]} (${nodeVramTotalGb || 24} GB)`;
		}
		return "Host CPU (Modo Mock)";
	}, [telemetry?.gpus, nodeVramTotalGb]);

	return {
		telemetry,
		nodeVramTotalGb,
		deviceLabel,
	};
}
