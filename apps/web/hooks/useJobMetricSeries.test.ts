import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import type { MetricPointWithKey } from "@/types/jobs";
import {
  initialMetricSeriesState,
  metricSeriesReducer,
  useJobMetricSeries,
  type UseJobMetricSeriesReturn,
} from "./useJobMetricSeries";

describe("useJobMetricSeries - Contract & Initial State", () => {
  it("initializes with empty points when no jobId is provided", () => {
    let captured: UseJobMetricSeriesReturn | null = null;

    function TestComponent() {
      const hookReturn = useJobMetricSeries(null);
      captured = hookReturn;
      return createElement("div", null, "test");
    }

    renderToString(createElement(TestComponent));

    expect(captured).not.toBeNull();
    if (!captured) return;

    const res = captured as UseJobMetricSeriesReturn;
    expect(res.points).toEqual([]);
    expect(res.maxSeq).toBe(0);
    expect(res.isLoading).toBe(false);
    expect(res.isDownsampled).toBe(false);
    expect(res.error).toBeNull();
    expect(typeof res.appendPoints).toBe("function");
    expect(typeof res.reset).toBe("function");
  });
});
const pt = (seq: number, step = seq): MetricPointWithKey => ({
  seq,
  epoch: 1,
  step,
  key: "loss",
  value: seq,
  ts: "2026-10-02T10:00:00Z",
});

describe("metricSeriesReducer", () => {
  it("keeps SSE points that arrive before the initial GET resolves", () => {
    let s = metricSeriesReducer(initialMetricSeriesState, { type: "start", jobId: "A", loading: true });
    s = metricSeriesReducer(s, { type: "append", points: [pt(5)], maxSeq: 5 });
    s = metricSeriesReducer(s, { type: "loaded", jobId: "A", points: [pt(1), pt(2), pt(3)], maxSeq: 3, downsampled: false });
    expect(s.points.map((p) => p.seq).sort((a, b) => a - b)).toEqual([1, 2, 3, 5]);
    expect(s.maxSeq).toBe(5);
    expect(s.isLoading).toBe(false);
  });

  it("does not mix points across a job switch", () => {
    let s = metricSeriesReducer(initialMetricSeriesState, { type: "start", jobId: "A", loading: true });
    s = metricSeriesReducer(s, { type: "append", points: [pt(7)], maxSeq: 7 });
    s = metricSeriesReducer(s, { type: "start", jobId: "B", loading: true });
    expect(s.points).toEqual([]);
    expect(s.maxSeq).toBe(0);
    // resposta atrasada do job A é descartada
    s = metricSeriesReducer(s, { type: "loaded", jobId: "A", points: [pt(1)], maxSeq: 1, downsampled: false });
    expect(s.points).toEqual([]);
    expect(s.isLoading).toBe(true);
    s = metricSeriesReducer(s, { type: "loaded", jobId: "B", points: [pt(2)], maxSeq: 2, downsampled: true });
    expect(s.points.map((p) => p.seq)).toEqual([2]);
    expect(s.isDownsampled).toBe(true);
  });
});
