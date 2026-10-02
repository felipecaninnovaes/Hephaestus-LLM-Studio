import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { ThermalBadge } from "@/components/ui/ThermalBadge";
import type { GpuDeviceTelemetry } from "@/lib/monitoring";
import { MultiGpuRack } from "./MultiGpuRack";

describe("ThermalBadge", () => {
	it("renders '—' and neutral styling when temperatureC is null or undefined", () => {
		const htmlNull = renderToString(
			createElement(ThermalBadge, { temperatureC: null }),
		);
		expect(htmlNull).toContain("—");
		expect(htmlNull).toContain("Temperatura indisponível");

		const htmlUndef = renderToString(
			createElement(ThermalBadge, { temperatureC: undefined }),
		);
		expect(htmlUndef).toContain("—");
	});

	it("renders neutral/brand styling for temperatures below 70°C", () => {
		const html = renderToString(
			createElement(ThermalBadge, { temperatureC: 62 }),
		);
		expect(html).toContain("62°C");
		expect(html).toContain("faixa normal");
		expect(html).toContain("text-brand-300");
	});

	it("renders intermediate alert styling (#f59e0b) for temperatures between 70°C and 80°C", () => {
		const html70 = renderToString(
			createElement(ThermalBadge, { temperatureC: 70 }),
		);
		expect(html70).toContain("70°C");
		expect(html70).toContain("faixa alta");
		expect(html70).toContain("text-status-alert");

		const html79 = renderToString(
			createElement(ThermalBadge, { temperatureC: 79 }),
		);
		expect(html79).toContain("79°C");
		expect(html79).toContain("faixa alta");
	});

	it("renders critical alert styling with glow for temperatures above 80°C", () => {
		const html85 = renderToString(
			createElement(ThermalBadge, { temperatureC: 85 }),
		);
		expect(html85).toContain("85°C");
		expect(html85).toContain("faixa crítica");
		expect(html85).toContain("shadow-[0_0_8px_rgba(245,158,11,0.25)]");
	});
});

describe("MultiGpuRack", () => {
	const dualGpuFixture: GpuDeviceTelemetry[] = [
		{
			index: 0,
			uuid: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
			name: "NVIDIA GeForce RTX 3060",
			vramTotal: 12288,
			vramUsed: 4096,
			powerWatts: 110,
			gpuUtilizationPct: 82,
			temperatureC: 65,
		},
		{
			index: 1,
			uuid: "GPU-c83cc056-07f7-d31e-cc98-7486ddac0296",
			name: "NVIDIA GeForce GTX 1660 SUPER",
			vramTotal: 6144,
			vramUsed: 1024,
			powerWatts: 75,
			gpuUtilizationPct: 15,
			temperatureC: 74,
		},
	];

	it("returns null when devices array is empty or null", () => {
		const htmlEmpty = renderToString(
			createElement(MultiGpuRack, { devices: [] }),
		);
		expect(htmlEmpty).toBe("");

		const htmlNull = renderToString(
			createElement(MultiGpuRack, { devices: null }),
		);
		expect(htmlNull).toBe("");
	});

	it("renders dual GPU rack with names, indices, VRAM bars and telemetry", () => {
		const html = renderToString(
			createElement(MultiGpuRack, { devices: dualGpuFixture }),
		);
		expect(html).toContain("RACK MULTI-GPU (2 PLACAS)");
		expect(html).toContain("GPU 0");
		expect(html).toContain("NVIDIA GeForce RTX 3060");
		expect(html).toContain("GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17");
		expect(html).toContain("4.0");
		expect(html).toContain("12.0 GB");
		expect(html).toContain("82%");
		expect(html).toContain("110W");
		expect(html).toContain("65°C");

		expect(html).toContain("GPU 1");
		expect(html).toContain("NVIDIA GeForce GTX 1660 SUPER");
		expect(html).toContain("GPU-c83cc056-07f7-d31e-cc98-7486ddac0296");
		expect(html).toContain("1.0");
		expect(html).toContain("6.0 GB");
		expect(html).toContain("15%");
		expect(html).toContain("75W");
		expect(html).toContain("74°C");
	});

	it("renders '—' for missing/null optional sensors (never 0)", () => {
		const devicesMissingSensors: GpuDeviceTelemetry[] = [
			{
				index: 0,
				uuid: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
				name: "NVIDIA GeForce RTX 3060",
				vramTotal: 12288,
				vramUsed: 4096,
				powerWatts: null,
				gpuUtilizationPct: null,
				temperatureC: null,
			},
		];

		const html = renderToString(
			createElement(MultiGpuRack, { devices: devicesMissingSensors }),
		);
		expect(html).toContain("UTIL:");
		expect(html).toContain("POWER:");
		expect(html).toContain("—");
		// Verify it didn't fabricate 0W or 0%
		expect(html).not.toContain("0W");
		expect(html).not.toContain('UTIL:</span><span aria-label="Utilização: 0%"');
	});
});
