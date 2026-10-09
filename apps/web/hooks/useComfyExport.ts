"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { showToast } from "@/components/ui/Toast";
import { isApiError } from "@/lib/api";
import {
	comfyErrorMessage,
	isFileExistsError,
	pollComfyExport,
} from "@/lib/comfyExport";
import { getComfyExport, startComfyExport } from "@/lib/comfyui";
import type {
	ComfyExport,
	ComfyExportSource,
	ComfyTarget,
} from "@/types/comfyui";

export interface ComfyExportState {
	phase: "idle" | "starting" | "running" | "done" | "failed";
	export: ComfyExport | null;
	/** Mensagem legível quando phase === "failed". */
	error: string | null;
	/** A falha foi `file_exists` — oferecer "Sobrescrever". */
	canOverwrite: boolean;
	/** Destino do último envio (para reenviar com overwrite). */
	target: ComfyTarget | null;
}

const IDLE: ComfyExportState = {
	phase: "idle",
	export: null,
	error: null,
	canOverwrite: false,
	target: null,
};

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Dispara um export e acompanha por polling de 1 s até done/failed, com toast no fim. */
export function useComfyExport(source: ComfyExportSource, label: string) {
	const [state, setState] = useState<ComfyExportState>(IDLE);
	const runRef = useRef(0);
	const sourceRef = useRef(source);
	sourceRef.current = source;

	useEffect(
		() => () => {
			runRef.current += 1; // cancela polling pendente ao desmontar
		},
		[],
	);

	const start = useCallback(
		async (target: ComfyTarget, overwrite = false) => {
			const run = ++runRef.current;
			const cancelled = () => run !== runRef.current;
			const fail = (error: string, canOverwrite: boolean) => {
				setState({
					phase: "failed",
					export: null,
					error,
					canOverwrite,
					target,
				});
				showToast(`${label}: ${error}`, "error");
			};
			setState({ ...IDLE, phase: "starting", target });
			try {
				const created = await startComfyExport(target.id, {
					...sourceRef.current,
					...(overwrite ? { overwrite: true } : {}),
				});
				if (cancelled()) return;
				setState({ ...IDLE, phase: "running", export: created, target });
				const result = await pollComfyExport(created.id, {
					getExport: getComfyExport,
					sleep,
					isCancelled: cancelled,
					onUpdate: (e) =>
						setState((s) => ({ ...s, phase: "running", export: e })),
				});
				if (result.kind === "cancelled") return;
				if (result.kind === "lost") {
					fail(
						"perdi o acompanhamento do envio — confira de novo mais tarde.",
						false,
					);
					return;
				}
				const e = result.export;
				if (e.status === "done") {
					setState({ ...IDLE, phase: "done", export: e, target });
					showToast(`${label} enviado ao ComfyUI (${target.name}).`, "success");
					return;
				}
				const exists = isFileExistsError(e.error);
				const msg = exists
					? "o arquivo já existe no ComfyUI."
					: (e.error ?? "falha desconhecida no envio.");
				setState({
					phase: "failed",
					export: e,
					error: msg,
					canOverwrite: exists,
					target,
				});
				showToast(`${label}: ${msg}`, "error");
			} catch (err) {
				if (cancelled()) return;
				if (isApiError(err)) {
					fail(comfyErrorMessage(err.status, err.code), isFileExistsError(err));
				} else {
					fail("falha ao iniciar o envio.", false);
				}
			}
		},
		[label],
	);

	const reset = useCallback(() => {
		runRef.current += 1;
		setState(IDLE);
	}, []);

	return { state, start, reset };
}
