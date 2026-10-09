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

/** Códigos conhecidos (node + BFF); o servidor pode emitir outros — trate como string aberta. */
export type ComfyExportErrorCode =
	| "file_exists"
	| "checksum_mismatch"
	| "size_mismatch"
	| "chunk_too_large"
	| "upload_not_found"
	| "token_not_configured"
	| "unauthorized"
	| "invalid_filename"
	| "invalid_size"
	| "invalid_body"
	| "invalid_offset"
	| "size_exceeded"
	| "network"
	| "remote_error"
	| "source_not_found"
	| "source_invalid"
	| "interrupted"
	| "internal"
	| "size_too_large"
	| "insufficient_storage"
	| "too_many_uploads"
	| "token_undecryptable"
	| (string & {});

export interface ComfyExport {
	id: string;
	targetId: string;
	targetName: string;
	filename: string;
	status: ComfyExportStatus;
	bytesTotal: number;
	bytesSent: number;
	error: string | null;
	errorCode: ComfyExportErrorCode | null;
	remotePath: string | null;
	createdAt: string;
	updatedAt: string;
}

/** Exatamente uma forma: modelo do catálogo OU artefato de job. */
export type ComfyExportSource =
	| { modelId: string }
	| { jobId: string; artifactId: string };

export type ComfyExportRequest = ComfyExportSource & { overwrite?: boolean };
