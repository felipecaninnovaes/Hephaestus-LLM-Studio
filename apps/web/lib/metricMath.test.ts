import { describe, expect, it } from "bun:test";
import type { MetricPointWithKey } from "@/types/jobs";
import {
	calculateEMA,
	computeGlobalSteps,
	dedupeMetricPoints,
	mergeMetricPoints,
	pivotMetricPoints,
} from "./metricMath";

describe("metricMath - pure functions", () => {
	describe("dedupeMetricPoints & mergeMetricPoints", () => {
		it("deduplicates points with identical seq and key", () => {
			const raw: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 0,
					step: 2,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:01Z",
				},
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				}, // dup
				{
					seq: 2,
					epoch: 0,
					step: 2,
					key: "lr",
					value: 0.001,
					ts: "2026-10-02T10:00:01Z",
				}, // same seq, different key
			];

			const deduped = dedupeMetricPoints(raw);
			expect(deduped.length).toBe(3);
			expect(deduped.map((p) => `${p.seq}-${p.key}`)).toEqual([
				"1-loss",
				"2-loss",
				"2-lr",
			]);
		});

		it("merges existing and incoming points seamlessly", () => {
			const existing: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 0,
					step: 2,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:01Z",
				},
			];
			const incoming: MetricPointWithKey[] = [
				{
					seq: 2,
					epoch: 0,
					step: 2,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:01Z",
				}, // overlap
				{
					seq: 3,
					epoch: 0,
					step: 3,
					key: "loss",
					value: 0.9,
					ts: "2026-10-02T10:00:02Z",
				},
			];

			const merged = mergeMetricPoints(existing, incoming);
			expect(merged.length).toBe(3);
			expect(merged[2].seq).toBe(3);
			expect(merged[2].value).toBe(0.9);
		});
	});

	describe("computeGlobalSteps", () => {
		it("orders steps monotonically across epochs when step restarts", () => {
			const points: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 2.0,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 0,
					step: 2,
					key: "loss",
					value: 1.8,
					ts: "2026-10-02T10:00:01Z",
				},
				{
					seq: 3,
					epoch: 1,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:02Z",
				},
				{
					seq: 4,
					epoch: 1,
					step: 2,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:03Z",
				},
			];

			const stepMap = computeGlobalSteps(points);
			expect(stepMap.get("0:1")).toBe(1);
			expect(stepMap.get("0:2")).toBe(2);
			expect(stepMap.get("1:1")).toBe(3);
			expect(stepMap.get("1:2")).toBe(4);
		});

		it("preserves continuous step values if step is already strictly increasing", () => {
			const points: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 1,
					step: 5,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 1,
					step: 10,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:01Z",
				},
				{
					seq: 3,
					epoch: 2,
					step: 15,
					key: "loss",
					value: 0.9,
					ts: "2026-10-02T10:00:02Z",
				},
			];

			const stepMap = computeGlobalSteps(points);
			expect(stepMap.get("1:5")).toBe(5);
			expect(stepMap.get("1:10")).toBe(10);
			expect(stepMap.get("2:15")).toBe(15);
		});
	});

	describe("calculateEMA", () => {
		it("returns raw values if smoothing <= 0", () => {
			const raw = [10, 20, 30, null];
			const ema = calculateEMA(raw, 0);
			expect(ema).toEqual([10, 20, 30, null]);
		});

		it("calculates exponential moving average correctly", () => {
			const raw = [10, 20, 30];
			const ema = calculateEMA(raw, 0.5);
			expect(ema[0]).toBeCloseTo(10, 5);
			expect(ema[1]).toBeCloseTo(15, 5);
			expect(ema[2]).toBeCloseTo(22.5, 5);
		});
	});

	describe("pivotMetricPoints", () => {
		it("discards series where all values are zero (e.g. legacy yolo metrics on diffusion job)", () => {
			const points: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 0,
					step: 1,
					key: "lr",
					value: 0.0001,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 3,
					epoch: 0,
					step: 1,
					key: "box_loss",
					value: 0.0,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 4,
					epoch: 0,
					step: 2,
					key: "loss",
					value: 1.2,
					ts: "2026-10-02T10:00:01Z",
				},
				{
					seq: 5,
					epoch: 0,
					step: 2,
					key: "box_loss",
					value: 0.0,
					ts: "2026-10-02T10:00:01Z",
				},
			];

			const pivoted = pivotMetricPoints(points, "global_step", {
				allowedKeys: ["loss", "lr", "box_loss"],
			});
			expect(pivoted.seriesKeys.includes("loss")).toBe(true);
			expect(pivoted.seriesKeys.includes("lr")).toBe(true);
			expect(pivoted.seriesKeys.includes("box_loss")).toBe(false); // zero-only discarded
		});

		it("discards non-training keys like progress and vram_used_gb", () => {
			const points: MetricPointWithKey[] = [
				{
					seq: 1,
					epoch: 0,
					step: 1,
					key: "loss",
					value: 1.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 2,
					epoch: 0,
					step: 1,
					key: "vram_used_gb",
					value: 14.5,
					ts: "2026-10-02T10:00:00Z",
				},
				{
					seq: 3,
					epoch: 0,
					step: 1,
					key: "progress",
					value: 0.5,
					ts: "2026-10-02T10:00:00Z",
				},
			];

			const pivoted = pivotMetricPoints(points, "global_step", {
				jobKind: "diffusion_train",
			});
			expect(pivoted.seriesKeys).toEqual(["loss"]);
		});
	});
});
