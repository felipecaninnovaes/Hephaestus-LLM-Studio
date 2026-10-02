"use client";

import { useCallback, useEffect, useState } from "react";
import { getJobMetricPoints } from "@/lib/jobs";
import { mergeMetricPoints } from "@/lib/metricMath";
import type { MetricPointWithKey } from "@/types/jobs";

export interface UseJobMetricSeriesOptions {
	enabled?: boolean;
	maxPoints?: number;
	keys?: string[];
	/** Se fornecido, faz append inicial ou sincronização */
	initialPoints?: MetricPointWithKey[];
}

export interface UseJobMetricSeriesReturn {
	points: MetricPointWithKey[];
	maxSeq: number;
	isLoading: boolean;
	isDownsampled: boolean;
	error: string | null;
	appendPoints: (newPoints: MetricPointWithKey[], newMaxSeq?: number) => void;
	reset: () => void;
}

/**
 * Hook para carregar série de métricas brutas e receber atualizações incrementais via SSE (fatia 4a).
 * - Carga inicial via GET /api/jobs/:id/metrics?maxPoints=...&keys=...
 * - Recebe deltas ao vivo via appendPoints (chamado pelo evento SSE `metrics` de useJobTelemetry)
 * - Faz deduplicação estrita por seq + key sem refetch completo
 */
export function useJobMetricSeries(
	jobId: string | null | undefined,
	options: UseJobMetricSeriesOptions = {},
): UseJobMetricSeriesReturn {
	const { enabled = true, maxPoints = 2000, keys, initialPoints } = options;

	const [points, setPoints] = useState<MetricPointWithKey[]>(
		initialPoints ?? [],
	);
	const [maxSeq, setMaxSeq] = useState<number>(0);
	const [isLoading, setIsLoading] = useState<boolean>(false);
	const [isDownsampled, setIsDownsampled] = useState<boolean>(false);
	const [error, setError] = useState<string | null>(null);

	const keysString = keys?.slice().sort().join(",");

	const appendPoints = useCallback(
		(newPoints: MetricPointWithKey[], newMaxSeq?: number) => {
			if (!newPoints || newPoints.length === 0) return;
			setPoints((prev) => mergeMetricPoints(prev, newPoints));
			if (newMaxSeq != null) {
				setMaxSeq((prev) => Math.max(prev, newMaxSeq));
			} else {
				const batchMax = Math.max(...newPoints.map((p) => Number(p.seq)));
				setMaxSeq((prev) => Math.max(prev, batchMax));
			}
		},
		[],
	);

	const reset = useCallback(() => {
		setPoints([]);
		setMaxSeq(0);
		setError(null);
		setIsDownsampled(false);
	}, []);

	useEffect(() => {
		if (!jobId || !enabled) {
			return;
		}

		let isCancelled = false;
		setIsLoading(true);
		setError(null);

		const keysArr = keysString ? keysString.split(",") : undefined;

		getJobMetricPoints(jobId, {
			maxPoints,
			keys: keysArr,
		})
			.then((res) => {
				if (isCancelled) return;
				setPoints(res.items ?? []);
				setMaxSeq(res.maxSeq ?? 0);
				setIsDownsampled(res.downsampled ?? false);
				setIsLoading(false);
			})
			.catch((err) => {
				if (isCancelled) return;
				setIsLoading(false);
				setError(err instanceof Error ? err.message : String(err));
			});

		return () => {
			isCancelled = true;
		};
	}, [jobId, enabled, maxPoints, keysString]);

	return {
		points,
		maxSeq,
		isLoading,
		isDownsampled,
		error,
		appendPoints,
		reset,
	};
}
