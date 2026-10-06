"use client";

import { useEffect, useState } from "react";

import { getJobArtifacts } from "@/lib/jobs";

/** Cache por job id (jobs terminais não mudam): evita refetch a cada render/seleção. */
const cache = new Map<string, Promise<boolean>>();

function fetchHasCaptions(jobId: string): Promise<boolean> {
	let p = cache.get(jobId);
	if (!p) {
		p = getJobArtifacts(jobId)
			.then((r) => r.items.some((a) => a.kind === "captions"))
			.catch(() => {
				cache.delete(jobId);
				return false;
			});
		cache.set(jobId, p);
	}
	return p;
}

/**
 * Jobs autolabel `failed`/`cancelled` podem ter legendas parciais persistidas.
 * Retorna true somente se `GET /api/jobs/:id/artifacts` contém kind "captions".
 * Não consulta nada para outros jobs; erro de fetch => false (sem UI).
 */
export function usePartialCaptions(
	job: { id: string; kind?: string; status: string } | null | undefined,
): boolean {
	const eligible =
		!!job &&
		job.kind === "autolabel" &&
		(job.status === "failed" || job.status === "cancelled");
	const id = eligible ? job.id : null;
	const [state, setState] = useState<{ id: string; has: boolean } | null>(null);

	useEffect(() => {
		if (!id) return;
		let alive = true;
		fetchHasCaptions(id).then((has) => {
			if (alive) setState({ id, has });
		});
		return () => {
			alive = false;
		};
	}, [id]);

	return !!id && state?.id === id && state.has;
}
