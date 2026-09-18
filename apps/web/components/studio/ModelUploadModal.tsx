"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { IconBox, IconUpload } from "@/components/icons";
import {
	Button,
	Modal,
	SegmentedControl,
	Select,
	showToast,
} from "@/components/ui";
import type { SelectOption } from "@/components/ui/Select";
import {
	abortModelUpload,
	completeModelUpload,
	initModelUpload,
	uploadModel,
	uploadModelPart,
} from "@/lib/models";
import { type Model, modelErrorMessage } from "@/types/studio";

interface ModelUploadModalProps {
	open: boolean;
	onClose: () => void;
	onUploaded: (model: Model) => void;
}

const ENGINE_OPTIONS = [
	{ id: "yolo", label: "YOLO (Detecção / Treino)" },
	{ id: "world", label: "YOLO-World (AutoTracker)" },
	{ id: "diffusion", label: "Difusão (Geração)" },
	{ id: "clip", label: "CLIP (Embeddings)" },
];

const KIND_OPTIONS: SelectOption<string>[] = [
	{ value: "", label: "Detectar automaticamente (recomendado)" },
	{ value: "lora", label: "LoRA" },
	{ value: "checkpoint", label: "Checkpoint (modelo completo)" },
	{ value: "text_encoder", label: "Text encoder (Qwen3 — FLUX.2)" },
];

const ARCH_OPTIONS: SelectOption<string>[] = [
	{ value: "", label: "Auto" },
	{ value: "sdxl", label: "SDXL" },
	{ value: "sd15", label: "SD 1.5" },
	{ value: "flux-2-klein-4b", label: "FLUX.2 Klein 4B" },
];
// Limite do chunked: acima de 96 MiB o proxy Next bufferizaria o multipart
// inteiro em RAM (OOM do next-server com 8 GB). Partes ≤ 96 MiB são seguras.
const CHUNKED_THRESHOLD_BYTES = 96 * 1024 * 1024;
const PART_CONCURRENCY = 2;

export default function ModelUploadModal({
	open,
	onClose,
	onUploaded,
}: ModelUploadModalProps) {
	const [file, setFile] = useState<File | null>(null);
	const [name, setName] = useState("");
	const [engine, setEngine] = useState<"yolo" | "world" | "diffusion" | "clip">(
		"yolo",
	);
	const [kind, setKind] = useState("");
	const [arch, setArch] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [progress, setProgress] = useState<{ done: number; total: number } | null>(
		null,
	);
	const inputRef = useRef<HTMLInputElement>(null);
	// Sessão chunked aberta (para abort em cancelamento/unmount). Só vive
	// durante o fluxo chunked; null no fluxo multipart único (inalterado).
	const uploadIdRef = useRef<string | null>(null);
	const cancelledRef = useRef(false);
	const isDiffusionSafetensors =
		engine === "diffusion" &&
		file !== null &&
		file.name.toLowerCase().endsWith(".safetensors");
	// Upload em partes (> 96 MiB) só atendido para yolo/world/diffusion —
	// fora disso o init devolveria 400 opaco; bloqueia o submit antes.
	const isChunked = file !== null && file.size > CHUNKED_THRESHOLD_BYTES;
	const isChunkedEngineSupported =
		engine === "yolo" || engine === "world" || engine === "diffusion";
	const chunkedEngineBlocked = isChunked && !isChunkedEngineSupported;
	const reset = useCallback(() => {
		setFile(null);
		setName("");
		setEngine("yolo");
		setKind("");
		setArch("");
		setError(null);
		setProgress(null);
	}, []);

	const handleClose = useCallback(() => {
		if (busy) return;
		reset();
		onClose();
	}, [busy, reset, onClose]);

	function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
		const f = e.target.files?.[0] ?? null;
		setFile(f);
		setError(null);
	}

	// Aborta a sessão chunked aberta se o modal desmontar no meio do upload
	// (cancelamento/unmount com sessão aberta). Best-effort silencioso.
	useEffect(() => {
		return () => {
			if (uploadIdRef.current) {
				abortModelUpload(uploadIdRef.current).catch(() => {});
				uploadIdRef.current = null;
			}
		};
	}, []);
	function toModelErrorMessage(err: unknown): string {
		let code = "";
		let message: string | undefined;
		if (err !== null && typeof err === "object") {
			if ("code" in err && typeof err.code === "string") code = err.code;
			if ("message" in err && typeof err.message === "string")
				message = err.message;
		}
		return modelErrorMessage(code, message);
	}
	/** Fluxo chunked (> 96 MiB): init → PUT partes (conc. 2, 1 retry) → complete. */
	async function uploadChunked(current: File): Promise<Model> {
		const kindHint =
			isDiffusionSafetensors && kind ? kind : undefined;
		const archHint =
			isDiffusionSafetensors && arch ? arch : undefined;
		// Decisão S1 (backend): init.name é o filename cru COM extensão
		// (validate_raw_filename + sanitize preservando ext, sem display-name
		// separado no chunked) — sempre file.name + hints; o complete preserva.
		// Limitação honesta: override digitado é ignorado no fluxo chunked.
		// totalParts otimista com o teto do contrato (96 MiB); o servidor
		// devolve o partSize canônico e valida totalParts contra size.
		const assumedTotalParts = Math.ceil(current.size / CHUNKED_THRESHOLD_BYTES);
		const init = await initModelUpload({
			name: current.name,
			engine,
			kind: kindHint,
			arch: archHint,
			size: current.size,
			totalParts: assumedTotalParts,
		});
		uploadIdRef.current = init.uploadId;
		const { partSize, totalParts } = init;
		setProgress({ done: 0, total: totalParts });
		let done = 0;
		let nextPart = 0;
		let failed: unknown = null;

		async function worker() {
			while (nextPart < totalParts) {
				if (cancelledRef.current || failed) break;
				const partNumber = nextPart++;
				const start = partNumber * partSize;
				const blob = current.slice(start, start + partSize);
				try {
					await uploadModelPart(init.uploadId, partNumber, blob);
				} catch (err) {
					// Retry simples por parte (1 retry) antes de falhar.
					if (!cancelledRef.current) {
						try {
							await uploadModelPart(init.uploadId, partNumber, blob);
						} catch (retryErr) {
							failed = retryErr;
							break;
						}
					} else {
						failed = err;
						break;
					}
				}
				done++;
				setProgress({ done, total: totalParts });
			}
		}

		const workers = Array.from(
			{ length: Math.min(PART_CONCURRENCY, totalParts) },
			() => worker(),
		);
		await Promise.all(workers);
		if (failed) throw failed;
		if (cancelledRef.current) throw new Error("cancelled");
		const model = await completeModelUpload(init.uploadId);
		uploadIdRef.current = null;
		return model;
	}

	async function handleSubmit() {
		if (!file) return;
		const current = file;
		cancelledRef.current = false;
		setBusy(true);
		setError(null);
		setProgress(null);
		try {
			const model =
				current.size > CHUNKED_THRESHOLD_BYTES
					? await uploadChunked(current)
					: await uploadModel({
							file: current,
							engine,
							name: name.trim() || undefined,
							kind: isDiffusionSafetensors && kind ? kind : undefined,
							arch: isDiffusionSafetensors && arch ? arch : undefined,
						});
			showToast("Modelo enviado com sucesso.", "success");
			onUploaded(model);
			reset();
			onClose();
		} catch (err: unknown) {
			// Abort best-effort silencioso da sessão chunked em qualquer falha.
			if (uploadIdRef.current) {
				await abortModelUpload(uploadIdRef.current).catch(() => {});
				uploadIdRef.current = null;
			}
			if (
				(err instanceof Error && err.message === "cancelled") ||
				cancelledRef.current
			) {
				setError("Upload cancelado.");
			} else {
				const friendly = toModelErrorMessage(err);
				setError(friendly);
				showToast(friendly, "error");
			}
		} finally {
			setBusy(false);
			setProgress(null);
		}
	}

	/** Cancelamento durante o chunked: sinaliza workers + aborta a sessão. */
	function handleCancel() {
		if (!busy) {
			handleClose();
			return;
		}
		cancelledRef.current = true;
		if (uploadIdRef.current) {
			abortModelUpload(uploadIdRef.current).catch(() => {});
			uploadIdRef.current = null;
		}
	}

	return (
		<Modal
			open={open}
			onClose={handleClose}
			title="Enviar pesos"
			description="Upload de arquivo .pt ou .safetensors"
			icon={<IconUpload className="h-4 w-4" />}
			maxWidth="md"
			busy={busy}
		>
			<div className="space-y-4">
				{/* File picker */}
				<div>
					<label
						htmlFor="model-upload-file"
						className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400"
					>
						Arquivo
					</label>
					<input
						ref={inputRef}
						id="model-upload-file"
						type="file"
						accept=".pt,.safetensors"
						onChange={handleFileChange}
						disabled={busy}
						className="sr-only"
					/>
					<button
						type="button"
						onClick={() => inputRef.current?.click()}
						disabled={busy}
						aria-label="Selecionar arquivo .pt ou .safetensors"
						className={`flex w-full items-center gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors cursor-pointer ${
							file
								? "border-brand-500/30 bg-brand-500/10"
								: "border-zinc-700/60 bg-black/40 hover:border-zinc-600 hover:bg-black/50"
						} disabled:opacity-55`}
					>
						<IconBox className="size-4 shrink-0 text-zinc-400" />
						<div className="min-w-0 flex-1">
							{file ? (
								<>
									<div className="truncate text-xs font-medium text-zinc-100">
										{file.name}
									</div>
									<div className="font-mono text-2xs text-zinc-400">
										{(file.size / (1024 * 1024)).toFixed(1)} MB
									</div>
								</>
							) : (
								<div className="text-xs text-zinc-400">
									Selecionar arquivo .pt ou .safetensors…
								</div>
							)}
						</div>
					</button>
				</div>

				{/* Nome opcional */}
				<div>
					<label
						htmlFor="model-upload-name"
						className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400"
					>
						Nome (opcional)
					</label>
					<input
						id="model-upload-name"
						type="text"
						value={name}
						onChange={(e) => setName(e.target.value)}
						placeholder="ex: model.safetensors ou best_v2.pt"
						disabled={busy}
						maxLength={255}
						className="w-full rounded-lg border border-zinc-700/60 bg-black/40 px-3 py-1.5 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-55"
					/>
					{isChunked && (
						<p className="mt-1.5 font-mono text-2xs text-zinc-500 leading-normal">
							Nome customizado não se aplica ao upload em partes — o arquivo será registrado como {file?.name}.
						</p>
					)}
				</div>

				{/* Engine selector */}
				<div>
					<span className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400">
						Engine / Tipo
					</span>
					<SegmentedControl
						options={ENGINE_OPTIONS}
						value={engine}
						onChange={(v) => {
							setEngine(v as "yolo" | "world" | "diffusion" | "clip");
							// Limpar classificação ao mudar de engine
							setKind("");
							setArch("");
						}}
						ariaLabel="Tipo de modelo"
						className="w-full justify-start"
					/>
				</div>

				{/* Classificação opcional — apenas para difusão + safetensors */}
				{isDiffusionSafetensors && (
					<div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3">
						<div className="flex items-center gap-2">
							<span className="font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400">
								Classificação (opcional)
							</span>
						</div>

						<Select
							id="model-upload-kind"
							label="Kind"
							options={KIND_OPTIONS}
							value={kind}
							onChange={(val) => {
								setKind(val);
								// text_encoder só admite flux-2-klein-4b (sniff autoritativo) — força o hint.
								if (val === "text_encoder") setArch("flux-2-klein-4b");
							}}
							placeholder="Selecione o kind…"
							disabled={busy}
							fontMono
							size="default"
						/>

						<Select
							id="model-upload-arch"
							label="Arquitetura"
							options={
								kind === "text_encoder"
									? ARCH_OPTIONS.filter(
											(o) => o.value === "" || o.value === "flux-2-klein-4b",
										)
									: ARCH_OPTIONS
							}
							value={arch}
							onChange={(val) => setArch(val)}
							placeholder="Selecione a arquitetura…"
							disabled={busy || kind === "text_encoder"}
							fontMono
							size="default"
						/>

						<p className="font-mono text-2xs text-zinc-500 leading-normal">
							O estúdio detecta a arquitetura pelo arquivo. Use os seletores só
							se o upload falhar com erro de classificação.
						</p>
					</div>
				)}

				{/* Progresso chunked (partes concluídas/total) */}
				{progress && (
					<div className="rounded-lg border border-brand-500/30 bg-brand-500/10 px-3 py-2">
						<div className="font-mono text-2xs text-zinc-300">
							Enviando parte {progress.done} de {progress.total}…
						</div>
						<div
							role="progressbar"
							aria-valuenow={progress.done}
							aria-valuemin={0}
							aria-valuemax={progress.total}
							className="mt-1.5 h-1 overflow-hidden rounded-full bg-white/10"
						>
							<div
								className="h-full rounded-full bg-brand-500 transition-[width]"
								style={{
									width: `${progress.total > 0 ? (progress.done / progress.total) * 100 : 0}%`,
								}}
							/>
						</div>
					</div>
				)}

				{/* Erro */}
				{error && (
					<div className="rounded-lg border border-status-danger/30 bg-status-danger/[0.08] px-3 py-2 text-xs text-status-danger">
						{error}
					</div>
				)}
				{/* Engine fora do chunked: aviso + submit desabilitado (evita 400 opaco do init). */}
				{chunkedEngineBlocked && (
					<p className="font-mono text-2xs text-zinc-500 leading-normal">
						Upload em partes disponível para modelos yolo/world/diffusion.
					</p>
				)}

				{/* Ações — One CTA */}
				<div className="flex items-center justify-end gap-2 pt-1">
					<Button
						type="button"
						variant="ghost"
						size="sm"
						onClick={progress ? handleCancel : handleClose}
						disabled={busy && !progress}
					>
						Cancelar
					</Button>
					<Button
						type="button"
						variant="primary"
						size="sm"
						onClick={handleSubmit}
						disabled={!file || busy || chunkedEngineBlocked}
						loading={busy}
					>
						Enviar
					</Button>
				</div>
			</div>
		</Modal>
	);
}
