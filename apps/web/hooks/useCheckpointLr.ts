"use client";

import { useEffect, useState } from "react";
import { getJobMetricPoints } from "@/lib/jobs";
import { lastLrAtEpoch } from "@/lib/lrResume";

/**
 * LR em que o checkpoint `epoch` do job de origem parou, via
 * `GET /api/jobs/:id/metrics?keys=lr`. `null` enquanto carrega, sem dado ou
 * em caso de falha (a UI cai para o texto neutro).
 */
export function useCheckpointLr(
	sourceJobId: string | undefined,
	epoch: number | undefined,
): number | null {
	const [state, setState] = useState<{ key: string; lr: number | null } | null>(
		null,
	);
	const key = sourceJobId && epoch ? `${sourceJobId}:${epoch}` : "";

	useEffect(() => {
		if (!sourceJobId || !epoch) return;
		const controller = new AbortController();
		getJobMetricPoints(sourceJobId, {
			keys: ["lr"],
			maxPoints: 10000,
			signal: controller.signal,
		})
			.then((res) => {
				if (controller.signal.aborted) return;
				setState({
					key: `${sourceJobId}:${epoch}`,
					lr: lastLrAtEpoch(res.items, epoch),
				});
			})
			.catch(() => {
				if (controller.signal.aborted) return;
				setState({ key: `${sourceJobId}:${epoch}`, lr: null });
			});
		return () => controller.abort();
	}, [sourceJobId, epoch]);

	return state && state.key === key ? state.lr : null;
}
