import { apiFetch } from "@/lib/api";
import type {
	ComfyExport,
	ComfyExportRequest,
	ComfyTarget,
	ComfyTargetCreateRequest,
	ComfyTargetPatchRequest,
} from "@/types/comfyui";

const BASE = "/api/integrations/comfyui";

export function listComfyTargets(): Promise<ComfyTarget[]> {
	return apiFetch<ComfyTarget[]>(`${BASE}/targets`);
}

export function createComfyTarget(
	body: ComfyTargetCreateRequest,
): Promise<ComfyTarget> {
	return apiFetch<ComfyTarget>(`${BASE}/targets`, { method: "POST", body });
}

export function updateComfyTarget(
	id: string,
	body: ComfyTargetPatchRequest,
): Promise<ComfyTarget> {
	return apiFetch<ComfyTarget>(`${BASE}/targets/${id}`, {
		method: "PATCH",
		body,
	});
}

export async function deleteComfyTarget(id: string): Promise<void> {
	await apiFetch(`${BASE}/targets/${id}`, { method: "DELETE" });
}

/** Falha de conexão volta 200 com lastCheckOk=false — não é erro HTTP. */
export function testComfyTarget(id: string): Promise<ComfyTarget> {
	return apiFetch<ComfyTarget>(`${BASE}/targets/${id}/test`, {
		method: "POST",
	});
}

export function startComfyExport(
	targetId: string,
	body: ComfyExportRequest,
): Promise<ComfyExport> {
	return apiFetch<ComfyExport>(`${BASE}/targets/${targetId}/exports`, {
		method: "POST",
		body,
	});
}

export function getComfyExport(id: string): Promise<ComfyExport> {
	return apiFetch<ComfyExport>(`${BASE}/exports/${id}`);
}
