"use client";

import { Button } from "@/components/ui/Button";
import { ProgressBar } from "@/components/ui/ProgressBar";
import type { ComfyExportState } from "@/hooks/useComfyExport";
import { exportPercent } from "@/lib/comfyExport";
import { formatBytes } from "@/lib/format";

interface ComfyExportStatusProps {
	state: ComfyExportState;
	onOverwrite: () => void;
}

/** Progresso, sucesso ou erro do envio (com "Sobrescrever" em file_exists). */
export function ComfyExportStatus({
	state,
	onOverwrite,
}: ComfyExportStatusProps) {
	const { phase, export: e } = state;
	if (phase === "idle") return null;

	if (phase === "starting" || phase === "running") {
		const total = e?.bytesTotal ?? 0;
		return (
			<div className="w-full space-y-1" aria-live="polite">
				<ProgressBar
					size="md"
					variant="brand"
					value={e ? exportPercent(e) : 0}
					aria-label="Progresso do envio ao ComfyUI"
				/>
				<div className="flex justify-between font-mono text-3xs text-zinc-400">
					<span>
						{phase === "starting" || e?.status === "queued"
							? "Na fila…"
							: `Enviando para ${state.target?.name ?? "ComfyUI"}`}
					</span>
					<span>
						{e && total > 0
							? `${formatBytes(e.bytesSent)} / ${formatBytes(total)}`
							: ""}
					</span>
				</div>
			</div>
		);
	}

	if (phase === "done") {
		return (
			<p
				className="w-full truncate font-mono text-3xs text-status-success"
				title={e?.remotePath ?? undefined}
				role="status"
			>
				Enviado ao ComfyUI{e?.remotePath ? ` · ${e.remotePath}` : ""}
			</p>
		);
	}

	return (
		<div className="w-full space-y-1.5" role="alert">
			<p className="break-words font-mono text-3xs text-rose-300">
				{state.canOverwrite
					? "O arquivo já existe no ComfyUI."
					: (state.error ?? "Falha no envio.")}
			</p>
			{state.canOverwrite && (
				<Button type="button" variant="warning" size="sm" onClick={onOverwrite}>
					Sobrescrever
				</Button>
			)}
		</div>
	);
}
