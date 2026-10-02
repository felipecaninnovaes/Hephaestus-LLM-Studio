import { describe, expect, it } from "bun:test";
import type { MetricPointWithKey } from "@/types/jobs";
import {
	calculateEMA,
	computeGlobalSteps,
	computeXScaleRange,
	dedupeMetricPoints,
	mergeMetricPoints,
	nextXRange,
	pivotMetricPoints,
	sanitizeForLog,
} from "./metricMath";

describe("sanitizeForLog", () => {
	it("turns non-positive and non-finite values into gaps, keeps positives", () => {
		expect(sanitizeForLog([0.5, 0, -1, null, Number.NaN, 2e-6])).toEqual([
			0.5,
			null,
			null,
			null,
			null,
			2e-6,
		]);
	});
});

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

	describe("computeXScaleRange & pivotMetricPoints mode transitions", () => {
		const points: MetricPointWithKey[] = [
			{
				seq: 1,
				epoch: 1,
				step: 100,
				key: "loss",
				value: 1.5,
				ts: "2026-10-02T10:00:00Z",
			},
			{
				seq: 2,
				epoch: 2,
				step: 200,
				key: "loss",
				value: 1.2,
				ts: "2026-10-02T10:10:00Z",
			},
			{
				seq: 3,
				epoch: 6,
				step: 1400,
				key: "loss",
				value: 0.6,
				ts: "2026-10-02T11:00:00Z",
			},
		];

		it("computes accurate range for global_step mode", () => {
			const stepPivoted = pivotMetricPoints(points, "global_step");
			expect(stepPivoted.xRange.min).toBe(100);
			expect(stepPivoted.xRange.max).toBe(1400);
		});

		it("computes accurate range for epoch mode", () => {
			const epochPivoted = pivotMetricPoints(points, "epoch");
			expect(epochPivoted.xRange.min).toBe(1);
			expect(epochPivoted.xRange.max).toBe(6);
		});

		it("computes accurate range for ts mode", () => {
			const tsPivoted = pivotMetricPoints(points, "ts");
			const t0 = Math.floor(Date.parse("2026-10-02T10:00:00Z") / 1000);
			const t1 = Math.floor(Date.parse("2026-10-02T11:00:00Z") / 1000);
			expect(tsPivoted.xRange.min).toBe(t0);
			expect(tsPivoted.xRange.max).toBe(t1);
		});

		it("handles single point edge case with appropriate padding", () => {
			const singleRange = computeXScaleRange([5], "epoch");
			expect(singleRange.min).toBe(4);
			expect(singleRange.max).toBe(6);
		});
	});

	describe("epoch mode fractional x", () => {
		const mk = (
			seq: number,
			epoch: number | null,
			step: number,
		): MetricPointWithKey => ({
			seq,
			epoch,
			step,
			key: "loss",
			value: 1 + seq,
			ts: `2026-10-02T10:00:${String(seq).padStart(2, "0")}Z`,
		});

		it("spreads points inside each epoch with global steps", () => {
			const pts = [mk(1, 1, 0), mk(2, 1, 50), mk(3, 2, 100), mk(4, 2, 150), mk(5, 3, 200)];
			const xs = pivotMetricPoints(pts, "epoch").xValues;
			// (step − min) / (max − min + 1): 50 / 51 dentro da época 1
			expect(xs[0]).toBe(1);
			expect(xs[1]).toBeCloseTo(1 + 50 / 51);
			expect(xs[2]).toBe(2);
			expect(xs[3]).toBeCloseTo(2 + 50 / 51);
			expect(xs[4]).toBe(3);
		});

		it("spreads points inside each epoch when step restarts per epoch", () => {
			const pts = [mk(1, 1, 1), mk(2, 1, 2), mk(3, 1, 3), mk(4, 2, 1), mk(5, 2, 2), mk(6, 2, 3)];
			const xs = pivotMetricPoints(pts, "epoch").xValues;
			expect(xs).toHaveLength(6);
			expect(xs[0]).toBe(1);
			expect(xs[1]).toBeCloseTo(1 + 1 / 3);
			expect(xs[2]).toBeCloseTo(1 + 2 / 3);
			expect(xs[3]).toBe(2);
			expect(xs[5]).toBeCloseTo(2 + 2 / 3);
		});

		it("drops points without epoch in epoch mode only", () => {
			const pts = [mk(1, null, 5), mk(2, 1, 10), mk(3, 1, 20)];
			const xs = pivotMetricPoints(pts, "epoch").xValues;
			expect(xs).toHaveLength(2);
			expect(xs[1]).toBeCloseTo(1 + 10 / 11);
			expect(pivotMetricPoints(pts, "global_step").xValues).toHaveLength(3);
		});
	});

	describe("nextXRange (auto-follow vs preserved zoom)", () => {
		const prevFull = { min: 100, max: 1400 };
		const nextFull = { min: 100, max: 1500 };

		it("follows new data when the user is at the full range", () => {
			expect(nextXRange({ ...prevFull }, prevFull, nextFull, false)).toEqual(nextFull);
		});

		it("preserves a zoomed range when new points arrive", () => {
			expect(nextXRange({ min: 400, max: 700 }, prevFull, nextFull, false)).toBeNull();
		});

		it("resets zoom on mode change, e.g. step zoom then epoch", () => {
			const epochFull = { min: 1, max: 6.9 };
			expect(nextXRange({ min: 400, max: 700 }, prevFull, epochFull, true)).toEqual(epochFull);
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
