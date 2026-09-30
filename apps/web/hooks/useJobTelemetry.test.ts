import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { useJobTelemetry, type UseJobTelemetryReturn } from "./useJobTelemetry";
import type { JobTelemetryEvent } from "@/types/studio";

describe("useJobTelemetry - Contract & Initial State", () => {
	it("initializes with null/empty states for all enriched fields", () => {
		let capturedTelemetry: UseJobTelemetryReturn | null = null;

		function TestComponent() {
			const telemetry = useJobTelemetry(null);
			capturedTelemetry = telemetry;
			return createElement("div", null, "test");
		}

		renderToString(createElement(TestComponent));

		expect(capturedTelemetry).not.toBeNull();
		if (!capturedTelemetry) return;

		const t = capturedTelemetry as UseJobTelemetryReturn;
		expect(t.phase).toBeNull();
		expect(t.progress).toBe(0);
		expect(t.vramUsedGb).toBeNull();
		expect(t.vramReservedGb).toBeNull();
		expect(t.stepTimeSeconds).toBeNull();
		expect(t.speed).toBeNull();
		expect(t.etaSeconds).toBeNull();
		expect(t.etaFormatted).toBeNull();
		expect(t.isLive).toBe(false);
		expect(t.isFinished).toBe(false);
	});

	it("conforms to JobTelemetryEvent contract with enriched telemetry fields", () => {
		const event: JobTelemetryEvent = {
			timestamp: "2026-09-25T12:00:00Z",
			phase: "training",
			phaseMessage: "Treinando época 1/10",
			progress: 0.15,
			step: 15,
			totalSteps: 100,
			epoch: 1,
			totalEpochs: 10,
			vramUsedGb: 14.2,
			vramReservedGb: 16.5,
			stepTimeSeconds: 2.34,
			speed: "2.3s/step",
			etaSeconds: 198,
			etaFormatted: "03:18",
			metrics: {
				loss: 0.042,
				loss_ema: 0.045,
			},
		};

		expect(event.vramReservedGb).toBe(16.5);
		expect(event.stepTimeSeconds).toBe(2.34);
		expect(event.speed).toBe("2.3s/step");
		expect(event.etaSeconds).toBe(198);
		expect(event.etaFormatted).toBe("03:18");
	});
});
