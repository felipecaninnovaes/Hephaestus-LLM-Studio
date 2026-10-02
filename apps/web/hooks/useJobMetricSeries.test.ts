import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { useJobMetricSeries, type UseJobMetricSeriesReturn } from "./useJobMetricSeries";

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
