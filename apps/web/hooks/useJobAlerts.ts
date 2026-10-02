"use client";

import { useCallback, useEffect, useState } from "react";
import { applyAlertsSnapshot } from "@/lib/jobAlerts";
import { getJobAlerts } from "@/lib/jobs";
import type { JobAlert } from "@/types/jobs";

export interface UseJobAlertsReturn {
	/** Alertas do job (ativos e resolvidos). `null` enquanto a carga inicial não chegou. */
	alerts: JobAlert[] | null;
	isLoading: boolean;
	error: string | null;
	/** Substitui o snapshot inteiro — usado pelo evento SSE `alerts` (idempotente, fatia 3c). */
	applySnapshot: (items: JobAlert[]) => void;
}

/**
 * Carrega o snapshot de alertas de um job (`GET /api/jobs/:id/alerts`, fatia 3c) e
 * expõe `applySnapshot` para que o chamador conecte o evento SSE `alerts` de
 * `useJobTelemetry` (substituição completa da lista, nunca acumula).
 */
export function useJobAlerts(
	jobId: string | null | undefined,
): UseJobAlertsReturn {
	const [alerts, setAlerts] = useState<JobAlert[] | null>(null);
	const [isLoading, setIsLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		if (!jobId) {
			setAlerts(null);
			setError(null);
			return;
		}
		const controller = new AbortController();
		setIsLoading(true);
		setAlerts(null);
		getJobAlerts(jobId, { signal: controller.signal })
			.then((res) => {
				if (controller.signal.aborted) return;
				setAlerts(res.items ?? []);
				setError(null);
			})
			.catch((err) => {
				if (controller.signal.aborted) return;
				setError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				if (!controller.signal.aborted) setIsLoading(false);
			});
		return () => controller.abort();
	}, [jobId]);

	const applySnapshot = useCallback((items: JobAlert[]) => {
		setAlerts((prev) => applyAlertsSnapshot(prev, items));
	}, []);

	return { alerts, isLoading, error, applySnapshot };
}
