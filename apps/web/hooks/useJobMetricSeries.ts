"use client";

import { useCallback, useEffect, useReducer } from "react";
import { getJobMetricPoints } from "@/lib/jobs";
import { mergeMetricPoints } from "@/lib/metricMath";
import type { MetricPointWithKey } from "@/types/jobs";

export interface UseJobMetricSeriesOptions {
	enabled?: boolean;
	maxPoints?: number;
	keys?: string[];
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

export interface MetricSeriesState {
	jobId: string | null;
	points: MetricPointWithKey[];
	maxSeq: number;
	isLoading: boolean;
	isDownsampled: boolean;
	error: string | null;
}

export type MetricSeriesAction =
	| { type: "start"; jobId: string | null; loading: boolean }
	| { type: "append"; points: MetricPointWithKey[]; maxSeq?: number }
	| {
			type: "loaded";
			jobId: string;
			points: MetricPointWithKey[];
			maxSeq: number;
			downsampled: boolean;
	  }
	| { type: "failed"; jobId: string; error: string };

export const initialMetricSeriesState: MetricSeriesState = {
	jobId: null,
	points: [],
	maxSeq: 0,
	isLoading: false,
	isDownsampled: false,
	error: null,
};

/**
 * Estado da série de métricas (fatia 4a).
 * - `start` zera tudo para o job novo (nada do job anterior vaza).
 * - `loaded` faz merge com os deltas SSE recebidos durante o GET inicial e é
 *   descartado se pertence a outro job.
 */
export function metricSeriesReducer(
	state: MetricSeriesState,
	action: MetricSeriesAction,
): MetricSeriesState {
	switch (action.type) {
		case "start":
			return {
				...initialMetricSeriesState,
				jobId: action.jobId,
				isLoading: action.loading,
			};
		case "append": {
			if (action.points.length === 0) return state;
			const batchMax =
				action.maxSeq ??
				Math.max(...action.points.map((p) => Number(p.seq)));
			return {
				...state,
				points: mergeMetricPoints(state.points, action.points),
				maxSeq: Math.max(state.maxSeq, batchMax),
			};
		}
		case "loaded":
			if (action.jobId !== state.jobId) return state;
			return {
				...state,
				points: mergeMetricPoints(action.points, state.points),
				maxSeq: Math.max(state.maxSeq, action.maxSeq),
				isDownsampled: action.downsampled,
				isLoading: false,
			};
		case "failed":
			if (action.jobId !== state.jobId) return state;
			return { ...state, isLoading: false, error: action.error };
	}
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
	const { enabled = true, maxPoints = 2000, keys } = options;
	const [state, dispatch] = useReducer(
		metricSeriesReducer,
		initialMetricSeriesState,
	);

	const keysString = keys?.slice().sort().join(",");

	const appendPoints = useCallback(
		(newPoints: MetricPointWithKey[], newMaxSeq?: number) => {
			if (!newPoints || newPoints.length === 0) return;
			dispatch({ type: "append", points: newPoints, maxSeq: newMaxSeq });
		},
		[],
	);

	const reset = useCallback(() => {
		dispatch({ type: "start", jobId: null, loading: false });
	}, []);

	useEffect(() => {
		const active = !!jobId && enabled;
		dispatch({ type: "start", jobId: jobId ?? null, loading: active });
		if (!jobId || !active) return;

		const controller = new AbortController();
		getJobMetricPoints(jobId, {
			maxPoints,
			keys: keysString ? keysString.split(",") : undefined,
			signal: controller.signal,
		})
			.then((res) => {
				if (controller.signal.aborted) return;
				dispatch({
					type: "loaded",
					jobId,
					points: res.items ?? [],
					maxSeq: res.maxSeq ?? 0,
					downsampled: res.downsampled ?? false,
				});
			})
			.catch((err) => {
				if (controller.signal.aborted) return;
				dispatch({
					type: "failed",
					jobId,
					error: err instanceof Error ? err.message : String(err),
				});
			});

		return () => controller.abort();
	}, [jobId, enabled, maxPoints, keysString]);

	return {
		points: state.points,
		maxSeq: state.maxSeq,
		isLoading: state.isLoading,
		isDownsampled: state.isDownsampled,
		error: state.error,
		appendPoints,
		reset,
	};
}
