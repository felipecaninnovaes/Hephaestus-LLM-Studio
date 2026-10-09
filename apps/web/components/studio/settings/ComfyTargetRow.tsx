"use client";

import { IconPencil, IconRefresh, IconTrash } from "@/components/icons";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { targetCheckState } from "@/lib/comfyExport";
import { formatRelativeTime } from "@/lib/format";
import type { ComfyTarget } from "@/types/comfyui";

interface ComfyTargetRowProps {
	target: ComfyTarget;
	testing: boolean;
	onTest: (t: ComfyTarget) => void;
	onEdit: (t: ComfyTarget) => void;
	onDelete: (t: ComfyTarget) => void;
}

export function ComfyTargetRow({
	target: t,
	testing,
	onTest,
	onEdit,
	onDelete,
}: ComfyTargetRowProps) {
	const state = targetCheckState(t);
	return (
		<li className="flex flex-col gap-3 rounded-xl border border-white/10 bg-white/[0.02] p-4 sm:flex-row sm:items-center sm:justify-between">
			<div className="min-w-0 space-y-1.5">
				<div className="flex flex-wrap items-center gap-2">
					<h3 className="truncate font-display text-sm font-semibold text-zinc-100">
						{t.name}
					</h3>
					{state === "ok" && <Badge variant="ready">Conectado</Badge>}
					{state === "failed" && <Badge variant="danger">Falhou</Badge>}
					{state === "never" && <Badge variant="mono">Não testado</Badge>}
				</div>
				<p
					className="truncate font-mono text-2xs text-zinc-300"
					title={t.baseUrl}
				>
					{t.baseUrl}
				</p>
				<p className="font-mono text-3xs text-zinc-400">
					{t.hasToken ? "Token gravado" : "Sem token"}
					{t.lastCheckAt && ` · testado ${formatRelativeTime(t.lastCheckAt)}`}
				</p>
				{state === "failed" && t.lastCheckError && (
					<p className="break-words font-mono text-2xs text-rose-300">
						{t.lastCheckError}
					</p>
				)}
			</div>
			<div className="flex shrink-0 items-center gap-2">
				<Button
					type="button"
					variant="secondary"
					size="sm"
					loading={testing}
					onClick={() => onTest(t)}
				>
					{!testing && <IconRefresh className="size-3.5" />}
					Testar conexão
				</Button>
				<Button
					type="button"
					variant="ghost"
					size="sm"
					onClick={() => onEdit(t)}
					aria-label={`Editar destino ${t.name}`}
					title="Editar"
				>
					<IconPencil className="size-3.5" />
				</Button>
				<Button
					type="button"
					variant="ghost"
					size="sm"
					className="hover:bg-red-500/10 hover:text-red-400"
					onClick={() => onDelete(t)}
					aria-label={`Excluir destino ${t.name}`}
					title="Excluir"
				>
					<IconTrash className="size-3.5" />
				</Button>
			</div>
		</li>
	);
}
