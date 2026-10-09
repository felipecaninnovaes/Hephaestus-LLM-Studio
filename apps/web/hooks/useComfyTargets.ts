"use client";

import { useCallback, useEffect, useSyncExternalStore } from "react";
import { listComfyTargets } from "@/lib/comfyui";
import type { ComfyTarget } from "@/types/comfyui";

/* Cache de módulo: vários botões "Enviar ao ComfyUI" na mesma tela
   compartilham UMA requisição de lista; /settings invalida ao mutar. */

interface Snapshot {
	targets: ComfyTarget[];
	loading: boolean;
	loaded: boolean;
	error: boolean;
}

let snapshot: Snapshot = {
	targets: [],
	loading: false,
	loaded: false,
	error: false,
};
let inflight: Promise<void> | null = null;
const listeners = new Set<() => void>();

function set(next: Partial<Snapshot>) {
	snapshot = { ...snapshot, ...next };
	for (const l of listeners) l();
}

function load(): Promise<void> {
	if (inflight) return inflight;
	set({ loading: true });
	inflight = listComfyTargets()
		.then((targets) => set({ targets, loaded: true, error: false }))
		.catch(() => set({ error: true, loaded: true }))
		.finally(() => {
			inflight = null;
			set({ loading: false });
		});
	return inflight;
}

export function setComfyTargetsCache(targets: ComfyTarget[]) {
	set({ targets, loaded: true, error: false });
}

function subscribe(l: () => void) {
	listeners.add(l);
	return () => {
		listeners.delete(l);
	};
}

const getSnapshot = () => snapshot;

export function useComfyTargets(opts: { forceRefresh?: boolean } = {}) {
	const state = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
	const { forceRefresh = false } = opts;

	useEffect(() => {
		if (forceRefresh || !snapshot.loaded) void load();
	}, [forceRefresh]);

	const reload = useCallback(() => load(), []);
	return { ...state, reload };
}
