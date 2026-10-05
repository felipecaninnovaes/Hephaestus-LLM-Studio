"use client";

import { useSyncExternalStore, useCallback } from "react";
import { getTelemetry } from "@/lib/jobs";
import type { Telemetry } from "@/types/studio";

interface Subscriber {
	listener: () => void;
	intervalMs: number;
}

type IntervalHandle = number | NodeJS.Timeout;

let subscribers: Subscriber[] = [];
let currentTelemetry: Telemetry | null = null;
let timerId: IntervalHandle | null = null;
let currentIntervalMs: number | null = null;
let visibilityListenerAttached = false;
let isFetching = false;

async function fetchTelemetry() {
	if (typeof document !== "undefined" && document.visibilityState === "hidden") {
		return;
	}
	if (isFetching) {
		return;
	}
	isFetching = true;
	try {
		const data = await getTelemetry();
		currentTelemetry = data;
		for (const sub of subscribers) {
			sub.listener();
		}
	} catch {
		// Falhas esparsas ignoradas silenciosamente no polling
	} finally {
		isFetching = false;
	}
}

function handleVisibilityChange() {
	if (typeof document !== "undefined" && document.visibilityState === "visible") {
		void fetchTelemetry();
		reconfigureTimer();
	} else {
		clearTimer();
	}
}

function clearTimer() {
	if (timerId !== null) {
		clearInterval(timerId);
		timerId = null;
	}
	currentIntervalMs = null;
}

function reconfigureTimer() {
	if (typeof document !== "undefined" && document.visibilityState === "hidden") {
		clearTimer();
		return;
	}

	if (subscribers.length === 0) {
		clearTimer();
		return;
	}

	const shortest = Math.min(...subscribers.map((s) => s.intervalMs));

	if (timerId !== null && currentIntervalMs === shortest) {
		return;
	}

	clearTimer();
	currentIntervalMs = shortest;
	timerId = setInterval(() => {
		void fetchTelemetry();
	}, shortest);
}

export function subscribeTelemetry(
	listener: () => void,
	intervalMs = 3000,
): () => void {
	const sub: Subscriber = { listener, intervalMs };
	const wasEmpty = subscribers.length === 0;
	subscribers.push(sub);

	if (
		typeof document !== "undefined" &&
		!visibilityListenerAttached &&
		typeof document.addEventListener === "function"
	) {
		document.addEventListener("visibilitychange", handleVisibilityChange);
		visibilityListenerAttached = true;
	}

	if (wasEmpty) {
		void fetchTelemetry();
	}

	reconfigureTimer();

	return () => {
		subscribers = subscribers.filter((s) => s !== sub);
		if (subscribers.length === 0) {
			clearTimer();
			if (
				typeof document !== "undefined" &&
				visibilityListenerAttached &&
				typeof document.removeEventListener === "function"
			) {
				document.removeEventListener("visibilitychange", handleVisibilityChange);
				visibilityListenerAttached = false;
			}
		} else {
			reconfigureTimer();
		}
	};
}

export function getTelemetrySnapshot(): Telemetry | null {
	return currentTelemetry;
}

export function resetTelemetryStoreForTest() {
	clearTimer();
	subscribers = [];
	currentTelemetry = null;
	isFetching = false;
	if (
		typeof document !== "undefined" &&
		visibilityListenerAttached &&
		typeof document.removeEventListener === "function"
	) {
		document.removeEventListener("visibilitychange", handleVisibilityChange);
	}
	visibilityListenerAttached = false;
}

export interface UseSharedTelemetryOptions {
	intervalMs?: number;
	enabled?: boolean;
}

/**
 * Hook reativo para assinar a telemetria compartilhada do nó.
 * Compartilha um único timer/tick entre todos os consumidores com refcount e intervalo mínimo.
 */
export function useSharedTelemetry(
	intervalOrOptions: number | UseSharedTelemetryOptions = 3000,
): Telemetry | null {
	const intervalMs =
		typeof intervalOrOptions === "number"
			? intervalOrOptions
			: (intervalOrOptions.intervalMs ?? 3000);
	const enabled =
		typeof intervalOrOptions === "object"
			? (intervalOrOptions.enabled ?? true)
			: true;

	const subscribe = useCallback(
		(listener: () => void) => {
			if (!enabled) return () => {};
			return subscribeTelemetry(listener, intervalMs);
		},
		[enabled, intervalMs],
	);

	return useSyncExternalStore(subscribe, getTelemetrySnapshot, () => null);
}
