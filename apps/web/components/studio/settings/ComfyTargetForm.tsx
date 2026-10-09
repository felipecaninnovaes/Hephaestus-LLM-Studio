"use client";

import { useEffect, useState } from "react";
import { IconServer } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Modal } from "@/components/ui/Modal";
import { isApiError } from "@/lib/api";
import {
	buildTargetPatch,
	comfyErrorMessage,
	normalizeBaseUrl,
	type TargetFormErrors,
	type TargetFormValues,
	validateTargetForm,
} from "@/lib/comfyExport";
import { createComfyTarget, updateComfyTarget } from "@/lib/comfyui";
import type { ComfyTarget } from "@/types/comfyui";

interface ComfyTargetFormProps {
	open: boolean;
	/** null = criar; destino = editar. */
	target: ComfyTarget | null;
	onClose: () => void;
	onSaved: (t: ComfyTarget) => void;
}

const EMPTY: TargetFormValues = { name: "", baseUrl: "", token: "" };

export function ComfyTargetForm({
	open,
	target,
	onClose,
	onSaved,
}: ComfyTargetFormProps) {
	const [values, setValues] = useState<TargetFormValues>(EMPTY);
	const [errors, setErrors] = useState<TargetFormErrors>({});
	const [formError, setFormError] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);
	const editing = target !== null;

	useEffect(() => {
		if (!open) return;
		setValues(
			target
				? { name: target.name, baseUrl: target.baseUrl, token: "" }
				: EMPTY,
		);
		setErrors({});
		setFormError(null);
	}, [open, target]);

	const set =
		(k: keyof TargetFormValues) => (e: React.ChangeEvent<HTMLInputElement>) =>
			setValues((v) => ({ ...v, [k]: e.target.value }));

	async function submit(e: React.FormEvent) {
		e.preventDefault();
		const found = validateTargetForm(values, Boolean(target?.hasToken));
		setErrors(found);
		setFormError(null);
		if (Object.keys(found).length > 0) return;
		setBusy(true);
		try {
			if (target) {
				const patch = buildTargetPatch(target, values);
				if (Object.keys(patch).length === 0) {
					onClose();
					return;
				}
				onSaved(await updateComfyTarget(target.id, patch));
			} else {
				onSaved(
					await createComfyTarget({
						name: values.name.trim(),
						baseUrl: normalizeBaseUrl(values.baseUrl),
						token: values.token.trim(),
					}),
				);
			}
			onClose();
		} catch (err) {
			if (isApiError(err) && err.code === "comfy_target_name_taken") {
				setErrors({ name: comfyErrorMessage(err.status, err.code) });
			} else if (isApiError(err)) {
				setFormError(comfyErrorMessage(err.status, err.code));
			} else {
				setFormError("Falha ao salvar o destino.");
			}
		} finally {
			setBusy(false);
		}
	}

	return (
		<Modal
			open={open}
			onClose={onClose}
			title={editing ? "Editar destino ComfyUI" : "Novo destino ComfyUI"}
			description="O token fica gravado cifrado e nunca é exibido de novo."
			icon={<IconServer />}
			maxWidth="md"
			busy={busy}
		>
			<form onSubmit={(e) => void submit(e)} className="space-y-4" noValidate>
				<Input
					label="Nome"
					value={values.name}
					onChange={set("name")}
					error={errors.name}
					placeholder="RunPod 4090"
					autoComplete="off"
				/>
				<Input
					label="URL base"
					value={values.baseUrl}
					onChange={set("baseUrl")}
					error={errors.baseUrl}
					placeholder="https://abc123-8188.proxy.runpod.net"
					fontMono
					autoComplete="off"
					inputMode="url"
				/>
				<Input
					label="Token"
					type="password"
					value={values.token}
					onChange={set("token")}
					error={errors.token}
					placeholder={editing ? "••••••••" : "Token do custom node"}
					hint={
						editing
							? "Deixe vazio para manter o token atual."
							: "O mesmo valor de HEPHAESTUS_COMFY_TOKEN no ComfyUI."
					}
					fontMono
					autoComplete="new-password"
				/>
				{formError && (
					<p role="alert" className="font-mono text-2xs text-rose-300">
						{formError}
					</p>
				)}
				<div className="flex justify-end gap-2 pt-1">
					<Button
						type="button"
						variant="ghost"
						onClick={onClose}
						disabled={busy}
					>
						Cancelar
					</Button>
					<Button type="submit" variant="primary" size="lg" loading={busy}>
						{editing ? "Salvar" : "Adicionar destino"}
					</Button>
				</div>
			</form>
		</Modal>
	);
}
