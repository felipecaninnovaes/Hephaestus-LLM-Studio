/* Integração ComfyUI — wire camelCase de /api/integrations/comfyui/* (contrato §2). */

export interface ComfyTarget {
	id: string;
	name: string;
	baseUrl: string;
	/** O token nunca volta na resposta — só a flag de que existe um gravado. */
	hasToken: boolean;
	lastCheckAt: string | null;
	lastCheckOk: boolean | null;
	lastCheckError: string | null;
	createdAt: string;
	updatedAt: string;
}

export interface ComfyTargetCreateRequest {
	name: string;
	baseUrl: string;
	token: string;
}

export interface ComfyTargetPatchRequest {
	name?: string;
	baseUrl?: string;
	token?: string;
}

export type ComfyExportStatus = "queued" | "uploading" | "done" | "failed";

export interface ComfyExport {
	id: string;
	targetId: string;
	targetName: string;
	filename: string;
	status: ComfyExportStatus;
	bytesTotal: number;
	bytesSent: number;
	error: string | null;
	remotePath: string | null;
	createdAt: string;
	updatedAt: string;
}

/** Exatamente uma forma: modelo do catálogo OU artefato de job. */
export type ComfyExportSource =
	| { modelId: string }
	| { jobId: string; artifactId: string };

export type ComfyExportRequest = ComfyExportSource & { overwrite?: boolean };
