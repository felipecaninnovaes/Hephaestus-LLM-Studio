import { describe, expect, it } from "bun:test";
import {
	buildCompareCacheKey,
	useJobsMetricsCompare,
} from "./useJobsMetricsCompare";

describe("useJobsMetricsCompare cache", () => {
	it("buildCompareCacheKey inclui jobId, keys normalizadas e maxPoints", () => {
		const key1 = buildCompareCacheKey("job-123", ["loss", "lr"], 2500);
		expect(key1).toBe("job-123|loss,lr|2500");

		// Ordem das chaves é normalizada
		const key2 = buildCompareCacheKey("job-123", ["lr", "loss"], 2500);
		expect(key2).toBe("job-123|loss,lr|2500");

		// String pré-ordenada
		const key3 = buildCompareCacheKey("job-123", "loss,lr", 2500);
		expect(key3).toBe("job-123|loss,lr|2500");
	});

	it("chaves diferentes geram cache keys distintas", () => {
		const keyA = buildCompareCacheKey("job-1", ["loss"], 2500);
		const keyB = buildCompareCacheKey("job-1", ["lr"], 2500);
		const keyC = buildCompareCacheKey("job-2", ["loss"], 2500);
		const keyD = buildCompareCacheKey("job-1", ["loss"], 5000);

		expect(keyA).not.toBe(keyB);
		expect(keyA).not.toBe(keyC);
		expect(keyA).not.toBe(keyD);
	});

	it("lida com keys undefined ou vazias", () => {
		const keyEmpty = buildCompareCacheKey("job-1", undefined, 2500);
		expect(keyEmpty).toBe("job-1||2500");

		const keyArrEmpty = buildCompareCacheKey("job-1", [], 2500);
		expect(keyArrEmpty).toBe("job-1||2500");
	});

	it("exporta useJobsMetricsCompare como função React hook", () => {
		expect(typeof useJobsMetricsCompare).toBe("function");
	});
});
