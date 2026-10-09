"use client";

import { useState } from "react";
import { IconPlus, IconServer } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { EmptyState } from "@/components/ui/EmptyState";
import { showToast } from "@/components/ui/Toast";
import { setComfyTargetsCache, useComfyTargets } from "@/hooks/useComfyTargets";
import { isApiError } from "@/lib/api";
import { comfyErrorMessage } from "@/lib/comfyExport";
import { deleteComfyTarget, testComfyTarget } from "@/lib/comfyui";
import type { ComfyTarget } from "@/types/comfyui";
import { ComfyInstallHint } from "./ComfyInstallHint";
import { ComfyTargetForm } from "./ComfyTargetForm";
import { ComfyTargetRow } from "./ComfyTargetRow";

export function ComfyTargetsSection() {
	const { targets, loaded, error, reload } = useComfyTargets({
		forceRefresh: true,
	});
	const [formOpen, setFormOpen] = useState(false);
	const [editing, setEditing] = useState<ComfyTarget | null>(null);
	const [deleting, setDeleting] = useState<ComfyTarget | null>(null);
	const [deleteBusy, setDeleteBusy] = useState(false);
	const [testingId, setTestingId] = useState<string | null>(null);

	const openForm = (t: ComfyTarget | null) => {
		setEditing(t);
		setFormOpen(true);
	};

	function upsert(saved: ComfyTarget) {
		const exists = targets.some((t) => t.id === saved.id);
		setComfyTargetsCache(
			exists
				? targets.map((t) => (t.id === saved.id ? saved : t))
				: [...targets, saved],
		);
		showToast(
			exists ? "Destino atualizado." : "Destino adicionado.",
			"success",
		);
	}

	async function test(t: ComfyTarget) {
		setTestingId(t.id);
		try {
			const res = await testComfyTarget(t.id);
			setComfyTargetsCache(targets.map((x) => (x.id === res.id ? res : x)));
			showToast(
				res.lastCheckOk
					? `${t.name}: conexão OK.`
					: `${t.name}: falha na conexão.`,
				res.lastCheckOk ? "success" : "error",
			);
		} catch (err) {
			showToast(
				isApiError(err)
					? comfyErrorMessage(err.status, err.code)
					: "Falha ao testar.",
				"error",
			);
		} finally {
			setTestingId(null);
		}
	}

	async function confirmDelete() {
		if (!deleting) return;
		setDeleteBusy(true);
		try {
			await deleteComfyTarget(deleting.id);
			setComfyTargetsCache(targets.filter((t) => t.id !== deleting.id));
			showToast("Destino removido.", "success");
			setDeleting(null);
		} catch (err) {
			showToast(
				isApiError(err)
					? comfyErrorMessage(err.status, err.code)
					: "Falha ao remover.",
				"error",
			);
		} finally {
			setDeleteBusy(false);
		}
	}

	return (
		<section className="space-y-4" aria-labelledby="comfyui-heading">
			<div className="flex items-start justify-between gap-3">
				<div>
					<h2
						id="comfyui-heading"
						className="font-display text-base font-semibold text-white"
					>
						Integrações · ComfyUI
					</h2>
					<p className="mt-0.5 text-xs text-zinc-400">
						Envie LoRAs treinados para um ComfyUI remoto com um clique.
					</p>
				</div>
				{targets.length > 0 && (
					<Button
						type="button"
						variant="primary"
						size="sm"
						onClick={() => openForm(null)}
					>
						<IconPlus className="size-3.5" />
						Novo destino
					</Button>
				)}
			</div>

			<ComfyInstallHint />

			{!loaded ? (
				<div className="h-24 animate-pulse rounded-xl bg-white/[0.04]" />
			) : error && targets.length === 0 ? (
				<EmptyState
					title="Falha ao carregar destinos"
					description="Não foi possível listar os destinos ComfyUI."
					actionLabel="Tentar novamente"
					onAction={() => void reload()}
				/>
			) : targets.length === 0 ? (
				<EmptyState
					icon={<IconServer />}
					title="Nenhum destino cadastrado"
					description="Cadastre a URL do ComfyUI e o token do custom node para habilitar “Enviar ao ComfyUI”."
					actionLabel="Adicionar destino"
					actionVariant="primary"
					onAction={() => openForm(null)}
				/>
			) : (
				<ul className="space-y-2.5">
					{targets.map((t) => (
						<ComfyTargetRow
							key={t.id}
							target={t}
							testing={testingId === t.id}
							onTest={(x) => void test(x)}
							onEdit={openForm}
							onDelete={setDeleting}
						/>
					))}
				</ul>
			)}

			<ComfyTargetForm
				open={formOpen}
				target={editing}
				onClose={() => setFormOpen(false)}
				onSaved={upsert}
			/>
			<ConfirmDialog
				open={deleting !== null}
				title="Excluir destino"
				body={
					<p>
						Excluir{" "}
						<strong className="font-mono text-zinc-100">
							{deleting?.name}
						</strong>
						? O histórico de envios para ele também será removido.
					</p>
				}
				confirmLabel="Excluir"
				danger
				busy={deleteBusy}
				onConfirm={() => void confirmDelete()}
				onClose={() => setDeleting(null)}
			/>
		</section>
	);
}
