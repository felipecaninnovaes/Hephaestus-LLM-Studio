import type {
	ComfyExport,
	ComfyExportStatus,
	ComfyTarget,
} from "@/types/comfyui";
import type { Job, JobArtifact } from "@/types/jobs";
import type { Model } from "@/types/models";

/* ── Escolha de destino ─────────────────────────────────────────────── */

export type ExportChoice =
	| { kind: "none" }
	| { kind: "direct"; target: ComfyTarget }
	| { kind: "menu"; targets: ComfyTarget[] };

/** Sem destino → link p/ /settings; 1 → envia direto; vários → menu. */
export function chooseExportTarget(targets: ComfyTarget[]): ExportChoice {
	if (targets.length === 0) return { kind: "none" };
	if (targets.length === 1) return { kind: "direct", target: targets[0] };
	return { kind: "menu", targets };
}

/* ── Elegibilidade (espelha o 422 do BFF: só LoRA .safetensors de difusão) ── */

export function isComfyExportableModel(model: Pick<Model, "engine" | "kind">) {
	return model.engine === "diffusion" && model.kind === "lora";
}

export function isComfyExportableArtifact(
	job: Pick<Job, "kind" | "engine">,
	art: Pick<JobArtifact, "path">,
): boolean {
	const kind = String(job.kind);
	const isDiffusionTrain =
		kind === "diffusion" ||
		kind === "diffusion_train" ||
		(job.engine === "diffusion" && kind !== "diffusion_generate");
	return isDiffusionTrain && art.path.toLowerCase().endsWith(".safetensors");
}

/* ── Estado do export ──────────────────────────────────────────────── */

export function isTerminalExport(status: ComfyExportStatus): boolean {
	return status === "done" || status === "failed";
}

export function exportPercent(
	e: Pick<ComfyExport, "status" | "bytesSent" | "bytesTotal">,
): number {
	if (e.status === "done") return 100;
	if (!(e.bytesTotal > 0)) return 0;
	return Math.min(100, Math.max(0, (e.bytesSent / e.bytesTotal) * 100));
}

/** `file_exists` chega só como export `failed` com `errorCode` (o POST é sempre 202). */
export function isFileExistsFailure(
	e: Pick<ComfyExport, "status" | "errorCode">,
): boolean {
	return e.status === "failed" && e.errorCode === "file_exists";
}

/* ── Polling ───────────────────────────────────────────────────────── */

export interface PollOptions {
	getExport: (id: string) => Promise<ComfyExport>;
	sleep: (ms: number) => Promise<void>;
	onUpdate: (e: ComfyExport) => void;
	/** Consultado antes de cada espera/consulta; true encerra sem resultado. */
	isCancelled?: () => boolean;
	intervalMs?: number;
	/** Falhas de rede consecutivas toleradas antes de desistir. */
	maxConsecutiveErrors?: number;
}

export type PollResult =
	| { kind: "terminal"; export: ComfyExport }
	| { kind: "cancelled" }
	| { kind: "lost"; error: unknown };

/** Consulta o export a cada `intervalMs` até `done`/`failed`. Erros transitórios não derrubam. */
export async function pollComfyExport(
	id: string,
	opts: PollOptions,
): Promise<PollResult> {
	const interval = opts.intervalMs ?? 1000;
	const maxErrors = opts.maxConsecutiveErrors ?? 5;
	let errors = 0;
	for (;;) {
		if (opts.isCancelled?.()) return { kind: "cancelled" };
		try {
			const e = await opts.getExport(id);
			if (opts.isCancelled?.()) return { kind: "cancelled" };
			errors = 0;
			opts.onUpdate(e);
			if (isTerminalExport(e.status)) return { kind: "terminal", export: e };
		} catch (error) {
			errors += 1;
			if (errors >= maxErrors) return { kind: "lost", error };
		}
		await opts.sleep(interval);
	}
}

/* ── Destinos: formulário ──────────────────────────────────────────── */

export interface TargetFormValues {
	name: string;
	baseUrl: string;
	token: string;
}

export type TargetFormErrors = Partial<Record<keyof TargetFormValues, string>>;

/** Remove espaços e barras finais (o BFF rejeita barra final). */
export function normalizeBaseUrl(raw: string): string {
	return raw.trim().replace(/\/+$/, "");
}

function baseUrlError(url: string): string | null {
	if (!url) return "Informe a URL do ComfyUI.";
	let parsed: URL;
	try {
		parsed = new URL(url);
	} catch {
		return "URL inválida. Exemplo: http://192.168.0.20:8188";
	}
	if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
		return "Use http:// ou https://.";
	}
	if (parsed.username || parsed.password) {
		return "Não coloque usuário/senha na URL — use o campo de token.";
	}
	return null;
}

export const MIN_TOKEN_LENGTH = 16;

/** `hadToken`=true (edição) torna o token opcional ("deixe vazio para manter"). */
export function validateTargetForm(
	v: TargetFormValues,
	hadToken: boolean,
): TargetFormErrors {
	const errors: TargetFormErrors = {};
	if (!v.name.trim()) errors.name = "Informe um nome.";
	const urlErr = baseUrlError(normalizeBaseUrl(v.baseUrl));
	if (urlErr) errors.baseUrl = urlErr;
	const token = v.token.trim();
	if (!hadToken && !token) errors.token = "Informe o token.";
	else if (token && token.length < MIN_TOKEN_LENGTH)
		errors.token = `O token precisa ter pelo menos ${MIN_TOKEN_LENGTH} caracteres.`;
	return errors;
}

/** Só os campos alterados; token vazio = manter o atual. Objeto vazio = nada a salvar. */
export function buildTargetPatch(
	original: Pick<ComfyTarget, "name" | "baseUrl">,
	v: TargetFormValues,
): { name?: string; baseUrl?: string; token?: string } {
	const patch: { name?: string; baseUrl?: string; token?: string } = {};
	const name = v.name.trim();
	const baseUrl = normalizeBaseUrl(v.baseUrl);
	if (name !== original.name) patch.name = name;
	if (baseUrl !== original.baseUrl) patch.baseUrl = baseUrl;
	if (v.token.trim()) patch.token = v.token.trim();
	return patch;
}

export type TargetCheckState = "never" | "ok" | "failed";

export function targetCheckState(
	t: Pick<ComfyTarget, "lastCheckAt" | "lastCheckOk">,
): TargetCheckState {
	if (t.lastCheckOk === true) return "ok";
	if (t.lastCheckOk === false) return "failed";
	return "never";
}

/* ── Mensagens ─────────────────────────────────────────────────────── */

const EXPORT_ERROR_MESSAGES: Record<string, string> = {
	file_exists: "o arquivo já existe no ComfyUI.",
	checksum_mismatch:
		"o arquivo chegou corrompido ao ComfyUI (checksum diferente).",
	size_mismatch: "o tamanho do arquivo no ComfyUI não confere.",
	chunk_too_large: "o ComfyUI recusou uma parte grande demais.",
	upload_not_found: "o ComfyUI perdeu a sessão de envio.",
	token_not_configured: "o custom node não tem token configurado no ComfyUI.",
	unauthorized: "o ComfyUI recusou o token do destino.",
	invalid_filename: "nome de arquivo inválido para o ComfyUI.",
	invalid_size: "tamanho de arquivo inválido.",
	invalid_body: "requisição inválida ao ComfyUI.",
	invalid_offset: "posição de envio inválida; tente de novo.",
	size_exceeded: "o arquivo excede o tamanho declarado.",
	network: "sem conexão com o ComfyUI.",
	remote_error: "o ComfyUI respondeu com erro após várias tentativas.",
	source_not_found: "o arquivo de origem não foi encontrado no armazenamento.",
	source_invalid:
		"o arquivo de origem é inválido (vazio, safetensors corrompido ou tamanho divergente).",
	interrupted: "envio interrompido por reinício do servidor.",
	internal: "erro interno ao enviar.",
	size_too_large: "o arquivo é grande demais para este destino.",
	insufficient_storage: "o ComfyUI não tem espaço em disco suficiente.",
	too_many_uploads:
		"o ComfyUI está com envios demais em andamento; tente de novo em instantes.",
	token_undecryptable:
		"o token deste destino ficou ilegível (o segredo do servidor mudou). Edite o destino em Configurações e informe o token de novo.",
};

/** Mensagem PT-BR por `errorCode`; código desconhecido cai na mensagem do servidor. */
export function exportFailureMessage(
	e: Pick<ComfyExport, "error" | "errorCode">,
): string {
	const known = e.errorCode ? EXPORT_ERROR_MESSAGES[e.errorCode] : undefined;
	return known ?? e.error ?? "falha desconhecida no envio.";
}

export function comfyErrorMessage(status: number, code: string): string {
	if (code === "comfy_target_name_taken")
		return "Já existe um destino com esse nome.";
	if (code === "export_unsupported" || status === 422)
		return "Só LoRAs de difusão em .safetensors podem ser enviados ao ComfyUI.";
	if (code === "queue_unavailable" || status === 503)
		return "Serviço indisponível — tente novamente.";
	if (status === 404) return "Destino ou item não encontrado.";
	if (status === 400) return "Parâmetros inválidos.";
	return "Falha na integração com o ComfyUI.";
}
