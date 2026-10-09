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

/** `file_exists` do custom node chega ao BFF como falha do export (ou 409 no POST). */
export function isFileExistsError(
	err: { code?: string } | string | null | undefined,
): boolean {
	if (!err) return false;
	if (typeof err === "string") return /file_exists|já existe/i.test(err);
	return err.code === "file_exists";
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

/** `hadToken`=true (edição) torna o token opcional ("deixe vazio para manter"). */
export function validateTargetForm(
	v: TargetFormValues,
	hadToken: boolean,
): TargetFormErrors {
	const errors: TargetFormErrors = {};
	if (!v.name.trim()) errors.name = "Informe um nome.";
	const urlErr = baseUrlError(normalizeBaseUrl(v.baseUrl));
	if (urlErr) errors.baseUrl = urlErr;
	if (!hadToken && !v.token.trim()) errors.token = "Informe o token.";
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

export function comfyErrorMessage(status: number, code: string): string {
	if (code === "file_exists") return "O arquivo já existe no ComfyUI.";
	if (status === 409) return "Já existe um destino com esse nome.";
	if (status === 422)
		return "Só LoRAs de difusão em .safetensors podem ser enviados ao ComfyUI.";
	if (status === 404) return "Destino ou item não encontrado.";
	if (status === 400) return "Parâmetros inválidos.";
	return "Falha na integração com o ComfyUI.";
}
