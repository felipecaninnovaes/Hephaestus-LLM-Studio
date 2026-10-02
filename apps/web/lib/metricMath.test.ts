import { describe, expect, it } from "bun:test";
import {
  calculateEMA,
  computeGlobalSteps,
  dedupeMetricPoints,
  mergeMetricPoints,
  pivotMetricPoints,
} from "./metricMath";
import type { MetricPointWithKey } from "@/types/jobs";

describe("metricMath - pure functions", () => {
  describe("dedupeMetricPoints & mergeMetricPoints", () => {
    it("deduplicates points with identical seq and key", () => {
      const raw: MetricPointWithKey[] = [
        { seq: 1, epoch: 0, step: 1, key: "loss", value: 1.5, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: 0, step: 2, key: "loss", value: 1.2, ts: "2026-10-02T10:00:01Z" },
        { seq: 1, epoch: 0, step: 1, key: "loss", value: 1.5, ts: "2026-10-02T10:00:00Z" }, // dup
        { seq: 2, epoch: 0, step: 2, key: "lr", value: 0.001, ts: "2026-10-02T10:00:01Z" }, // same seq, different key
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
        { seq: 1, epoch: 0, step: 1, key: "loss", value: 1.5, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: 0, step: 2, key: "loss", value: 1.2, ts: "2026-10-02T10:00:01Z" },
      ];
      const incoming: MetricPointWithKey[] = [
        { seq: 2, epoch: 0, step: 2, key: "loss", value: 1.2, ts: "2026-10-02T10:00:01Z" }, // overlap
        { seq: 3, epoch: 0, step: 3, key: "loss", value: 0.9, ts: "2026-10-02T10:00:02Z" },
      ];

      const merged = mergeMetricPoints(existing, incoming);
      expect(merged.length).toBe(3);
      expect(merged[2].seq).toBe(3);
      expect(merged[2].value).toBe(0.9);
    });
  });

  describe("computeGlobalSteps", () => {
    it("orders steps monotonically across epochs when step restarts", () => {
      // Época 0: steps 1, 2
      // Época 1: steps 1, 2 (step reiniciou!)
      const points: MetricPointWithKey[] = [
        { seq: 1, epoch: 0, step: 1, key: "loss", value: 2.0, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: 0, step: 2, key: "loss", value: 1.8, ts: "2026-10-02T10:00:01Z" },
        { seq: 3, epoch: 1, step: 1, key: "loss", value: 1.5, ts: "2026-10-02T10:00:02Z" },
        { seq: 4, epoch: 1, step: 2, key: "loss", value: 1.2, ts: "2026-10-02T10:00:03Z" },
      ];

      const stepMap = computeGlobalSteps(points);
      expect(stepMap.get("0:1")).toBe(1);
      expect(stepMap.get("0:2")).toBe(2);
      expect(stepMap.get("1:1")).toBe(3);
      expect(stepMap.get("1:2")).toBe(4);
    });

    it("handles null epochs properly", () => {
      const points: MetricPointWithKey[] = [
        { seq: 1, epoch: null, step: 1, key: "loss", value: 2.0, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: null, step: 2, key: "loss", value: 1.8, ts: "2026-10-02T10:00:01Z" },
      ];

      const stepMap = computeGlobalSteps(points);
      expect(stepMap.get("null:1")).toBe(1);
      expect(stepMap.get("null:2")).toBe(2);
    });
  });

  describe("calculateEMA", () => {
    it("returns raw values if smoothing <= 0", () => {
      const raw = [10, 20, 30, null];
      const ema = calculateEMA(raw, 0);
      expect(ema).toEqual([10, 20, 30, null]);
    });

    it("calculates exponential moving average correctly", () => {
      // smoothing = 0.5 => alpha = 0.5
      // pt 0: 10 => ema 10
      // pt 1: 20 => ema = 0.5*20 + 0.5*10 = 15
      // pt 2: 30 => ema = 0.5*30 + 0.5*15 = 22.5
      const raw = [10, 20, 30];
      const ema = calculateEMA(raw, 0.5);
      expect(ema[0]).toBeCloseTo(10, 5);
      expect(ema[1]).toBeCloseTo(15, 5);
      expect(ema[2]).toBeCloseTo(22.5, 5);
    });

    it("preserves nulls in middle of series", () => {
      const raw = [10, null, 20];
      const ema = calculateEMA(raw, 0.5);
      expect(ema[0]).toBeCloseTo(10, 5);
      expect(ema[1]).toBeNull();
      // next item resumes with alpha applied to previous valid EMA
      expect(ema[2]).toBeCloseTo(15, 5);
    });
  });

  describe("pivotMetricPoints", () => {
    it("pivots points into uPlot aligned structure with global_step", () => {
      const points: MetricPointWithKey[] = [
        { seq: 1, epoch: 0, step: 1, key: "loss", value: 2.0, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: 0, step: 1, key: "lr", value: 0.001, ts: "2026-10-02T10:00:00Z" },
        { seq: 3, epoch: 0, step: 2, key: "loss", value: 1.5, ts: "2026-10-02T10:00:05Z" },
        { seq: 4, epoch: 0, step: 2, key: "lr", value: 0.0009, ts: "2026-10-02T10:00:05Z" },
      ];

      const pivoted = pivotMetricPoints(points, "global_step");
      expect(pivoted.xValues.length).toBe(2);
      expect(pivoted.xValues).toEqual([1, 2]);
      expect(pivoted.seriesKeys.sort()).toEqual(["loss", "lr"].sort());
      expect(pivoted.seriesData.loss).toEqual([2.0, 1.5]);
      expect(pivoted.seriesData.lr).toEqual([0.001, 0.0009]);
    });

    it("ensures strict x monotonicity even if epochs repeat", () => {
      const points: MetricPointWithKey[] = [
        { seq: 1, epoch: 1, step: 1, key: "loss", value: 2.0, ts: "2026-10-02T10:00:00Z" },
        { seq: 2, epoch: 1, step: 2, key: "loss", value: 1.5, ts: "2026-10-02T10:00:01Z" },
        { seq: 3, epoch: 2, step: 1, key: "loss", value: 1.2, ts: "2026-10-02T10:00:02Z" },
      ];

      const pivoted = pivotMetricPoints(points, "epoch");
      expect(pivoted.xValues.length).toBe(3);
      // Every subsequent xValue must be strictly greater than predecessor
      for (let i = 1; i < pivoted.xValues.length; i++) {
        expect(pivoted.xValues[i]).toBeGreaterThan(pivoted.xValues[i - 1]);
      }
    });
  });
});
