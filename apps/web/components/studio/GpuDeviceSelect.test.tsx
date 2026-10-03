import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import type { GpuDeviceTelemetry } from "@/lib/monitoring";
import { buildGpuDeviceOptions, GpuDeviceSelect } from "./GpuDeviceSelect";

describe("GpuDeviceSelect", () => {
	const devicesFixture: GpuDeviceTelemetry[] = [
		{
			index: 0,
			uuid: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
			name: "NVIDIA GeForce RTX 3060",
			vramTotal: 12288, // 12 GB
			vramUsed: 4096,
			powerWatts: 115,
			gpuUtilizationPct: 78,
			temperatureC: 66,
		},
		{
			index: 1,
			uuid: "GPU-c83cc056-07f7-d31e-cc98-7486ddac0296",
			name: "NVIDIA GeForce GTX 1660 SUPER",
			vramTotal: 6144, // 6 GB
			vramUsed: 1024,
			powerWatts: null,
			gpuUtilizationPct: null,
			temperatureC: null,
		},
	];

	it("is disabled when orchestratorId is null or undefined", () => {
		const html = renderToString(
			createElement(GpuDeviceSelect, {
				orchestratorId: null,
				devices: devicesFixture,
				value: null,
				onChange: () => {},
			}),
		);
		expect(html).toContain("Selecione um nó específico");
		expect(html).toContain("disabled");
	});

	it("is disabled when orchestrator has no gpuDevices (CPU or mock)", () => {
		const html = renderToString(
			createElement(GpuDeviceSelect, {
				orchestratorId: "orch-cpu-1",
				devices: [],
				value: null,
				onChange: () => {},
			}),
		);
		expect(html).toContain("modo CPU/legado");
		expect(html).toContain("disabled");
	});

	it("renders Automatic option as default selected and shows placeholder when no value", () => {
		const html = renderToString(
			createElement(GpuDeviceSelect, {
				orchestratorId: "orch-gpu-1",
				devices: devicesFixture,
				value: null,
				onChange: () => {},
			}),
		);
		expect(html).toContain("Automático (menor placa que cabe)");
		expect(html).not.toContain('disabled=""');
	});

	it("displays selected GPU device label when value matches a device", () => {
		const html = renderToString(
			createElement(GpuDeviceSelect, {
				orchestratorId: "orch-gpu-1",
				devices: devicesFixture,
				value: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
				onChange: () => {},
			}),
		);
		expect(html).toContain("GPU 0: NVIDIA GeForce RTX 3060");
	});

	it("buildGpuDeviceOptions disables GPU with vramTotal < vramMinGb and keeps GPU with vramTotal >= vramMinGb enabled", () => {
		// RTX 3060 (12GB) >= 8 -> enabled; GTX 1660S (6GB) < 8 -> disabled
		const options = buildGpuDeviceOptions(devicesFixture, 8);
		expect(options).toHaveLength(3); // Auto + 2 GPUs

		const rtx3060Opt = options.find(
			(o) => o.value === "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
		);
		const gtx1660Opt = options.find(
			(o) => o.value === "GPU-c83cc056-07f7-d31e-cc98-7486ddac0296",
		);

		expect(rtx3060Opt).toBeDefined();
		expect(gtx1660Opt).toBeDefined();

		// RTX 3060 tem 12GB >= 8GB -> habilitada, sem disabledReason
		expect(rtx3060Opt?.disabled).toBe(false);
		expect(rtx3060Opt?.disabledReason).toBeUndefined();

		// GTX 1660S tem 6GB < 8GB -> desabilitada com motivo visível
		expect(gtx1660Opt?.disabled).toBe(true);
		expect(gtx1660Opt?.disabledReason).toBe(
			"Exige ≥8.0 GB VRAM (placa possui 6.0 GB)",
		);
	});
});
