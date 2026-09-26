import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { JobProgressLive } from "./JobProgressLive";

describe("JobProgressLive - Enriched Telemetry", () => {
	it("renders enriched VRAM with both used and reserved Gb", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				vramUsedGb: 12.34,
				vramReservedGb: 15.67,
			}),
		);

		expect(html).toContain("12.3 / 15.7 GB VRAM");
		expect(html).toContain("VRAM: 12.34 GB alocada / 15.67 GB reservada");
	});

	it("renders VRAM with only used Gb when reserved is absent", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				vramUsedGb: 10.5,
			}),
		);

		expect(html).toContain("10.5 GB VRAM");
		expect(html).toContain("VRAM: 10.50 GB alocada");
		expect(html).not.toContain("reservada");
	});

	it("renders VRAM with only reserved Gb when used is absent", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				vramReservedGb: 16.0,
			}),
		);

		expect(html).toContain("16.0 GB VRAM");
		expect(html).toContain("VRAM: 16.00 GB reservada");
	});

	it("does not render VRAM pill when both are null or 0", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				vramUsedGb: 0,
				vramReservedGb: null,
			}),
		);

		expect(html).not.toContain("GB VRAM");
	});

	it("prioritizes etaFormatted from engine telemetry", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				etaFormatted: "04:32",
				etaSeconds: 999,
				step: 5,
				totalSteps: 20,
			}),
		);

		expect(html).toContain("ETA ~04:32");
	});

	it("does not duplicate ETA prefix if etaFormatted already includes ETA", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				etaFormatted: "ETA 01:45",
			}),
		);

		expect(html).toContain("ETA 01:45");
		expect(html).not.toContain("ETA ~ETA");
	});

	it("falls back to etaSeconds when etaFormatted is absent", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				etaSeconds: 125, // 2m 5s
				step: 2,
				totalSteps: 10,
			}),
		);

		expect(html).toContain("ETA ~2m 5s");
	});

	it("displays speed badge with speed string", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				speed: "2.5 it/s",
			}),
		);

		expect(html).toContain("2.5 it/s");
		expect(html).toContain("Velocidade de processamento");
	});

	it("displays speed badge from stepTimeSeconds when speed is absent", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isLive: true,
				stepTimeSeconds: 4.87,
			}),
		);

		expect(html).toContain("4.9s/step");
	});

	it("hides speed badge when job isFinished", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isFinished: true,
				speed: "3.0s/step",
			}),
		);

		expect(html).not.toContain("3.0s/step");
	});

	it("renders properly in compact mode with enriched telemetry", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				compact: true,
				isLive: true,
				vramUsedGb: 8.2,
				vramReservedGb: 12.0,
				speed: "1.2s/step",
				etaFormatted: "00:50",
			}),
		);

		expect(html).toContain("8.2 / 12.0 GB VRAM");
		expect(html).toContain("1.2s/step");
		expect(html).toContain("ETA ~00:50");
	});

	it("handles null or undefined props gracefully without crashing", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				vramUsedGb: undefined,
				vramReservedGb: null,
				stepTimeSeconds: null,
				speed: null,
				etaSeconds: null,
				etaFormatted: null,
			}),
		);

		expect(html).toBeTruthy();
		expect(html).not.toContain("GB VRAM");
		expect(html).not.toContain("ETA");
	});

	it("extracts speed and ETA from phaseMessage when explicit props are absent", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				phaseMessage:
					"Época 2/5 · Step 18/75 · Loss: 0.3617 · 36.0s/step · ETA: 33m 26s",
			}),
		);

		expect(html).toContain("36.0s/step");
		expect(html).toContain("ETA ~33m 26s");
	});

	it("extracts speed and ETA from phaseMessage in compact mode", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				compact: true,
				phaseMessage:
					"Época 2/5 · Step 18/75 · Loss: 0.3617 · 36.0s/step · ETA: 33m 26s",
			}),
		);

		expect(html).toContain("36.0s/step");
		expect(html).toContain("ETA ~33m 26s");
	});

	it("ignores ETA: N/A in phaseMessage", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				phaseMessage:
					"Época 1/5 · Step 1/75 · 10.0s/step · ETA: N/A",
			}),
		);

		expect(html).toContain("10.0s/step");
		expect(html).not.toContain("ETA ~");
		expect(html).not.toContain("Tempo estimado restante de treino");
	});

	it("prioritizes explicit speed and etaFormatted over phaseMessage", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				speed: "1.5s/step",
				etaFormatted: "05:00",
				phaseMessage:
					"Época 2/5 · Step 18/75 · Loss: 0.3617 · 36.0s/step · ETA: 33m 26s",
			}),
		);

		expect(html).toContain("1.5s/step");
		expect(html).toContain("ETA ~05:00");
		expect(html).not.toContain("ETA ~33m 26s");
		expect(html).not.toContain('title="Velocidade de processamento">36.0s/step');
	});

	it("does not display speed or ETA badges from phaseMessage when job isFinished", () => {
		const html = renderToString(
			createElement(JobProgressLive, {
				isFinished: true,
				phaseMessage:
					"Época 5/5 · Step 75/75 · 36.0s/step · ETA: 00m 00s",
			}),
		);

		expect(html).not.toContain("Velocidade de processamento");
		expect(html).not.toContain("Tempo estimado restante de treino");
		expect(html).not.toContain("ETA ~");
	});
});
