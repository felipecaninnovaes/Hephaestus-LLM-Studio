"use client";

import { useEffect, useRef, useState } from "react";
import { getJob, getJobMetricPoints } from "@/lib/jobs";
import type { MetricPointWithKey } from "@/types/jobs";
import type { JobTelemetryEvent } from "@/types/studio";

export interface UseJobTelemetryOptions {
	enabled?: boolean;
	onFinished?: (event: JobTelemetryEvent | null) => void;
	onError?: (err: Error) => void;
	onMetricPoints?: (points: MetricPointWithKey[], maxSeq: number) => void;
	/** Último seq de métrica já carregado (delta no polling de fallback) */
	metricAfterSeq?: number;
	/** Chaves de métrica pedidas no polling de fallback */
	metricKeys?: string[];
}

/** Erros SSE consecutivos (sem `open` entre eles) antes de cair no polling. */
export const SSE_MAX_CONSECUTIVE_ERRORS = 3;
const EVENT_SOURCE_CLOSED = 2;

/**
 * Decide o que fazer num `onerror` do EventSource. Enquanto o navegador está
 * reconectando (CONNECTING), deixa a reconexão nativa agir — ela reenvia
 * `Last-Event-ID` e o servidor manda o delta. Cai no polling só se o stream
 * foi fechado de vez (CLOSED) ou após N falhas consecutivas.
 */
export function sseErrorAction(
	readyState: number,
	consecutiveErrors: number,
): "reconnect" | "fallback" {
	if (readyState === EVENT_SOURCE_CLOSED) return "fallback";
	return consecutiveErrors >= SSE_MAX_CONSECUTIVE_ERRORS
		? "fallback"
		: "reconnect";
}

export interface UseJobTelemetryReturn {
	phase: string | null;
	phaseMessage: string | null;
	progress: number;
	vramUsedGb: number | null;
	vramReservedGb: number | null;
	stepTimeSeconds: number | null;
	speed: string | null;
	etaSeconds: number | null;
	etaFormatted: string | null;
	step: number | null;
	totalSteps: number | null;
	epoch: number | null;
	totalEpochs: number | null;
	metrics: Record<string, unknown> | null;
	status: string | null;
	isLive: boolean;
	isFinished: boolean;
	error: string | null;
	lastEvent: JobTelemetryEvent | null;
}

/**
 * Hook reutilizável de telemetria em tempo real (ADR-0021 D4).
 * Conecta ao stream SSE (/api/jobs/:id/events) com fallback automático para polling.
 */
export function useJobTelemetry(
	jobId: string | null | undefined,
	options: UseJobTelemetryOptions = {},
): UseJobTelemetryReturn {
	const { enabled = true, onFinished, onError } = options;

	const [phase, setPhase] = useState<string | null>(null);
	const [phaseMessage, setPhaseMessage] = useState<string | null>(null);
	const [progress, setProgress] = useState<number>(0);
	const [vramUsedGb, setVramUsedGb] = useState<number | null>(null);
	const [vramReservedGb, setVramReservedGb] = useState<number | null>(null);
	const [stepTimeSeconds, setStepTimeSeconds] = useState<number | null>(null);
	const [speed, setSpeed] = useState<string | null>(null);
	const [etaSeconds, setEtaSeconds] = useState<number | null>(null);
	const [etaFormatted, setEtaFormatted] = useState<string | null>(null);
	const [step, setStep] = useState<number | null>(null);
	const [totalSteps, setTotalSteps] = useState<number | null>(null);
	const [epoch, setEpoch] = useState<number | null>(null);
	const [totalEpochs, setTotalEpochs] = useState<number | null>(null);
	const [metrics, setMetrics] = useState<Record<string, unknown> | null>(null);
	const [status, setStatus] = useState<string | null>(null);
	const [isLive, setIsLive] = useState<boolean>(false);
	const [isFinished, setIsFinished] = useState<boolean>(false);
	const [error, setError] = useState<string | null>(null);
	const [lastEvent, setLastEvent] = useState<JobTelemetryEvent | null>(null);

	const eventSourceRef = useRef<EventSource | null>(null);
	const pollTimerRef = useRef<NodeJS.Timeout | null>(null);
	const onFinishedRef = useRef(onFinished);
	onFinishedRef.current = onFinished;
	const onErrorRef = useRef(onError);
	onErrorRef.current = onError;
	const onMetricPointsRef = useRef(options.onMetricPoints);
	onMetricPointsRef.current = options.onMetricPoints;
	const metricSeqRef = useRef(0);
	metricSeqRef.current = Math.max(
		metricSeqRef.current,
		options.metricAfterSeq ?? 0,
	);
	const metricKeysRef = useRef(options.metricKeys);
	metricKeysRef.current = options.metricKeys;

	useEffect(() => {
		if (!jobId || !enabled) {
			setIsLive(false);
			return;
		}

		/* Novo job = estado zerado: sem isso a mensagem terminal do job anterior
       ("Geração finalizada… N/N imagens", isFinished, progress 1) congelava
       sobre o job novo até os primeiros eventos chegarem. */
		setPhase(null);
		setPhaseMessage(null);
		setProgress(0);
		setVramUsedGb(null);
		setVramReservedGb(null);
		setStepTimeSeconds(null);
		setSpeed(null);
		setEtaSeconds(null);
		setEtaFormatted(null);
		setStep(null);
		setTotalSteps(null);
		setEpoch(null);
		setTotalEpochs(null);
		setMetrics(null);
		setStatus(null);
		setIsLive(false);
		setIsFinished(false);
		setError(null);
		setLastEvent(null);
		// Seq do job anterior não vale aqui; o render seguinte reaplica metricAfterSeq
		metricSeqRef.current = 0;

		let isClosed = false;

		function applyEvent(ev: JobTelemetryEvent, terminal = false) {
			setLastEvent(ev);
			if (ev.phase) setPhase(ev.phase);
			if (ev.phaseMessage !== undefined) setPhaseMessage(ev.phaseMessage);
			if (typeof ev.progress === "number") setProgress(ev.progress);
			if (ev.vramUsedGb !== undefined) setVramUsedGb(ev.vramUsedGb);
			if (ev.vramReservedGb !== undefined) setVramReservedGb(ev.vramReservedGb);
			if (ev.stepTimeSeconds !== undefined)
				setStepTimeSeconds(ev.stepTimeSeconds);
			if (ev.speed !== undefined) setSpeed(ev.speed);
			if (ev.etaSeconds !== undefined) setEtaSeconds(ev.etaSeconds);
			if (ev.etaFormatted !== undefined) setEtaFormatted(ev.etaFormatted);
			if (ev.step !== undefined) setStep(ev.step);
			if (ev.totalSteps !== undefined) setTotalSteps(ev.totalSteps);
			if (ev.epoch !== undefined) setEpoch(ev.epoch);
			if (ev.totalEpochs !== undefined) setTotalEpochs(ev.totalEpochs);
			if (ev.metrics) setMetrics(ev.metrics as Record<string, unknown>);

			if (terminal) {
				setIsLive(false);
				setIsFinished(true);
				if (onFinishedRef.current) {
					onFinishedRef.current(ev);
				}
			}
		}

		function startPollingFallback() {
			if (isClosed || pollTimerRef.current) return;
			setIsLive(false);

			const pollMetrics = async () => {
				const onPoints = onMetricPointsRef.current;
				if (!onPoints) return;
				const res = await getJobMetricPoints(jobId as string, {
					afterSeq: metricSeqRef.current,
					keys: metricKeysRef.current,
				});
				if (isClosed || !res.items || res.items.length === 0) return;
				metricSeqRef.current = Math.max(metricSeqRef.current, res.maxSeq ?? 0);
				onPoints(res.items, res.maxSeq ?? 0);
			};

			const poll = async () => {
				if (isClosed) return;
				try {
					await pollMetrics();
					const j = await getJob(jobId as string);
					setStatus(j.status);
					/* Polling (fallback sem SSE): propaga só o que o job informa.
             JobResponse NÃO tem totalSteps/totalEpochs (só o SSE live tem) —
             sem eles o contador de imagens some em vez de mentir. Spreads
             condicionais evitam piscar contadores com nulls entre polls.
             Terminal `done` sem progress: fixa 1 (failed/cancelled mantêm
             o último valor; applyEvent ignora `undefined`). */
					const isTerm = ["done", "failed", "cancelled"].includes(j.status);
					const ev = {
						timestamp: new Date().toISOString(),
						phase: j.phase || j.status,
						phaseMessage: j.phaseMessage,
						...(typeof j.progress === "number" && Number.isFinite(j.progress)
							? { progress: j.progress }
							: j.status === "done"
								? { progress: 1 }
								: {}),
						...(j.step != null ? { step: j.step } : {}),
						...(j.epoch != null ? { epoch: j.epoch } : {}),
						...(j.vramUsedGb != null ? { vramUsedGb: j.vramUsedGb } : {}),
						...(j.vramReservedGb != null
							? { vramReservedGb: j.vramReservedGb }
							: {}),
					} as JobTelemetryEvent;
					applyEvent(ev, isTerm);
					if (isTerm) {
						if (pollTimerRef.current) {
							clearInterval(pollTimerRef.current);
							pollTimerRef.current = null;
						}
					}
				} catch (e) {
					const err = e instanceof Error ? e : new Error(String(e));
					setError(err.message);
					if (onErrorRef.current) onErrorRef.current(err);
				}
			};

			poll();
			pollTimerRef.current = setInterval(poll, 2000);
		}

		// Inicia conexão SSE
		try {
			const url = `/api/jobs/${jobId}/events`;
			const es = new EventSource(url);
			eventSourceRef.current = es;

			let consecutiveErrors = 0;
			es.onopen = () => {
				consecutiveErrors = 0;
				if (!isClosed) setIsLive(true);
			};

			es.addEventListener("snapshot", (e: MessageEvent) => {
				if (isClosed) return;
				try {
					const data: JobTelemetryEvent = JSON.parse(e.data);
					applyEvent(data, false);
				} catch {
					// Ignora JSON malformado
				}
			});

			es.addEventListener("telemetry", (e: MessageEvent) => {
				if (isClosed) return;
				try {
					const data: JobTelemetryEvent = JSON.parse(e.data);
					applyEvent(data, false);
				} catch {
					// Ignora JSON malformado
				}
			});

			es.addEventListener("metrics", (e: MessageEvent) => {
				if (isClosed) return;
				try {
					const data = JSON.parse(e.data);
					if (data && Array.isArray(data.items) && onMetricPointsRef.current) {
						metricSeqRef.current = Math.max(
							metricSeqRef.current,
							data.maxSeq ?? 0,
						);
						onMetricPointsRef.current(data.items, data.maxSeq ?? 0);
					}
				} catch {
					// Ignora JSON malformado
				}
			});

			es.addEventListener("finished", (e: MessageEvent) => {
				if (isClosed) return;
				try {
					const data: JobTelemetryEvent = JSON.parse(e.data);
					applyEvent(data, true);
				} catch {
					setIsFinished(true);
					setIsLive(false);
				}
				es.close();
			});

			es.onerror = () => {
				if (isClosed) return;
				consecutiveErrors += 1;
				if (sseErrorAction(es.readyState, consecutiveErrors) === "reconnect") {
					setIsLive(false);
					return;
				}
				es.close();
				eventSourceRef.current = null;
				startPollingFallback();
			};
		} catch {
			startPollingFallback();
		}

		return () => {
			isClosed = true;
			if (eventSourceRef.current) {
				eventSourceRef.current.close();
				eventSourceRef.current = null;
			}
			if (pollTimerRef.current) {
				clearInterval(pollTimerRef.current);
				pollTimerRef.current = null;
			}
		};
	}, [jobId, enabled]);

	return {
		phase,
		phaseMessage,
		progress,
		vramUsedGb,
		vramReservedGb,
		stepTimeSeconds,
		speed,
		etaSeconds,
		etaFormatted,
		step,
		totalSteps,
		epoch,
		totalEpochs,
		metrics,
		status,
		isLive,
		isFinished,
		error,
		lastEvent,
	};
}
