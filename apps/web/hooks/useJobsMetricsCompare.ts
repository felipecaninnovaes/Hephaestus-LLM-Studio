"use client";

import { useEffect, useState } from "react";
import {
	fetchJobsMetricPoints,
	type JobMetricsFetchResult,
} from "@/lib/jobMetrics";

export interface UseJobsMetricsCompareOptions {
	keys?: string[];
	maxPoints?: number;
	enabled?: boolean;
}

export interface UseJobsMetricsCompareReturn {
	/** Série de pontos brutos por `jobId` (fatia 4b). */
	seriesByJob: Record<string, JobMetricsFetchResult>;
	isLoading: boolean;
}

/** TTL do cache leve em memória: evita refetch ao reordenar/remontar a mesma seleção. */
const CACHE_TTL_MS = 5000;
const cache = new Map<string, { data: JobMetricsFetchResult; ts: number }>();

function cacheKey(jobId: string, keysKey: string, maxPoints: number): string {
	return `${jobId}|${keysKey}|${maxPoints}`;
}

/**
 * Busca métricas de múltiplos jobs em paralelo (fatia 4b) para a comparação
 * de runs (fatia 4c). Reaproveita `getJobMetricPoints` via
 * `fetchJobsMetricPoints` — uma requisição concorrente por job, todas
 * abortáveis em conjunto via `AbortController` compartilhado.
 */
export function useJobsMetricsCompare(
	jobIds: readonly string[],
	options: UseJobsMetricsCompareOptions = {},
): UseJobsMetricsCompareReturn {
	const { keys, maxPoints = 2500, enabled = true } = options;
	const [seriesByJob, setSeriesByJob] = useState<
		Record<string, JobMetricsFetchResult>
	>({});
	const [isLoading, setIsLoading] = useState(false);

	const idsKey = jobIds.slice().sort().join(",");
	const keysKey = keys?.slice().sort().join(",") ?? "";

	// biome-ignore lint/correctness/useExhaustiveDependencies: chaves derivadas (idsKey/keysKey) já cobrem jobIds/keys com string estável — ler os arrays originais reexecutaria a cada render
	useEffect(() => {
		if (!enabled || jobIds.length === 0) {
			setSeriesByJob({});
			return;
		}

		const now = Date.now();
		const toFetch: string[] = [];
		const cached: Record<string, JobMetricsFetchResult> = {};
		for (const jobId of jobIds) {
			const hit = cache.get(cacheKey(jobId, keysKey, maxPoints));
			if (hit && now - hit.ts < CACHE_TTL_MS) {
				cached[jobId] = hit.data;
			} else {
				toFetch.push(jobId);
			}
		}

		if (Object.keys(cached).length > 0) {
			setSeriesByJob((prev) => ({ ...prev, ...cached }));
		}
		if (toFetch.length === 0) {
			setIsLoading(false);
			return;
		}

		const controller = new AbortController();
		setIsLoading(true);
		fetchJobsMetricPoints(toFetch, {
			keys,
			maxPoints,
			signal: controller.signal,
		})
			.then((results) => {
				if (controller.signal.aborted) return;
				const next: Record<string, JobMetricsFetchResult> = {};
				for (const r of results) {
					next[r.jobId] = r;
					cache.set(cacheKey(r.jobId, keysKey, maxPoints), {
						data: r,
						ts: Date.now(),
					});
				}
				setSeriesByJob((prev) => ({ ...prev, ...next }));
			})
			.finally(() => {
				if (!controller.signal.aborted) setIsLoading(false);
			});

		return () => controller.abort();
	}, [idsKey, keysKey, maxPoints, enabled]);

	return { seriesByJob, isLoading };
}
