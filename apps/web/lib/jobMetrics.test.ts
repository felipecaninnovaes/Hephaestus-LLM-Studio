import { describe, expect, it } from "bun:test";
import {
	estimateTrainingEtaMs,
	isTrainingMetric,
	latestTrainingMetric,
	trainingMetrics,
} from "./jobMetrics";
import type { JobTelemetryEvent } from "@/types/studio";

describe("jobMetrics - estimateTrainingEtaMs", () => {
	it("estimates ETA correctly when totalSteps is provided explicitly", () => {
		const baseTime = Date.now();
		const events: JobTelemetryEvent[] = [
			{
				timestamp: new Date(baseTime).toISOString(),
				phase: "training",
				step: 10,
				totalSteps: 100,
				progress: 0.1,
			},
			{
				timestamp: new Date(baseTime + 10_000).toISOString(),
				phase: "training",
				step: 20,
				totalSteps: 100,
				progress: 0.2,
			},
		];

		const eta = estimateTrainingEtaMs(events);
		expect(eta).not.toBeNull();
		// 10s for 10 steps = 1s/step = 1000ms/step. 80 remaining steps = 80,000ms.
		expect(eta).toBe(80_000);
	});

	it("extracts totalSteps from phaseMessage when totalSteps is null", () => {
		const baseTime = Date.now();
		const events: JobTelemetryEvent[] = [
			{
				timestamp: new Date(baseTime).toISOString(),
				phase: "training",
				step: 10,
				totalSteps: null,
				phaseMessage:
					"Época 2/5 · Step 10/75 · Loss: 0.4500 · 30.0s/step · ETA: 32m 30s",
				progress: 0.13,
			},
			{
				timestamp: new Date(baseTime + 30_000).toISOString(),
				phase: "training",
				step: 11,
				totalSteps: null,
				phaseMessage:
					"Época 2/5 · Step 11/75 · Loss: 0.4200 · 30.0s/step · ETA: 32m 00s",
				progress: 0.15,
			},
		];

		const eta = estimateTrainingEtaMs(events);
		expect(eta).not.toBeNull();
		// 30s for 1 step = 30,000ms/step. 64 remaining steps = 1,920,000ms.
		expect(eta).toBe(1_920_000);
	});

	it("extracts totalSteps from message property when totalSteps is null", () => {
		const baseTime = Date.now();
		const events = [
			{
				timestamp: new Date(baseTime).toISOString(),
				phase: "training",
				step: 1,
				totalSteps: null,
				message: "Step 1/50 - Loss: 1.23",
				progress: 0.02,
			},
			{
				timestamp: new Date(baseTime + 5_000).toISOString(),
				phase: "training",
				step: 2,
				totalSteps: null,
				message: "Step 2/50 - Loss: 1.10",
				progress: 0.04,
			},
		] as unknown as JobTelemetryEvent[];

		const eta = estimateTrainingEtaMs(events);
		expect(eta).not.toBeNull();
		// 5s for 1 step = 5,000ms/step. 48 remaining steps = 240,000ms.
		expect(eta).toBe(240_000);
	});

	it("returns null when totalSteps is null and no Step pattern is present", () => {
		const baseTime = Date.now();
		const events: JobTelemetryEvent[] = [
			{
				timestamp: new Date(baseTime).toISOString(),
				phase: "training",
				step: 1,
				totalSteps: null,
				phaseMessage: "Downloading weights",
				progress: 0.01,
			},
			{
				timestamp: new Date(baseTime + 2_000).toISOString(),
				phase: "training",
				step: 2,
				totalSteps: null,
				phaseMessage: "Preparing batch",
				progress: 0.02,
			},
		];

		const eta = estimateTrainingEtaMs(events);
		expect(eta).toBeNull();
	});

	it("returns null when fewer than 2 points are provided", () => {
		expect(estimateTrainingEtaMs(null)).toBeNull();
		expect(estimateTrainingEtaMs([])).toBeNull();
		expect(
			estimateTrainingEtaMs([
				{
					timestamp: new Date().toISOString(),
					phase: "training",
					step: 1,
					totalSteps: 10,
					progress: 0.1,
				},
			]),
		).toBeNull();
	});
});

describe("jobMetrics - isTrainingMetric", () => {
	it("detects valid loss/lr as training metric", () => {
		expect(isTrainingMetric({ loss: 0.25 })).toBe(true);
		expect(isTrainingMetric({ lr: 0.0001 })).toBe(true);
		expect(isTrainingMetric({ loss: 0 })).toBe(true);
		expect(isTrainingMetric({})).toBe(false);
		expect(isTrainingMetric({ loss: Number.NaN })).toBe(false);
	});

	it("filters training metrics list", () => {
		const list = trainingMetrics([
			{ loss: 0.5 },
			{ message: "boot" } as unknown as { loss: number },
			{ lr: 0.001 },
		]);
		expect(list).toHaveLength(2);
	});

	it("finds latest training metric", () => {
		const latest = latestTrainingMetric([
			{ loss: 0.5 },
			{ loss: 0.3 },
			{ message: "done" } as unknown as { loss: number },
		]);
		expect(latest?.loss).toBe(0.3);
	});
});
