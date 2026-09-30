"use client";

import { useEffect, useRef, useState } from "react";
import { getJob } from "@/lib/jobs";
import type { JobTelemetryEvent } from "@/types/studio";

export interface UseJobTelemetryOptions {
  enabled?: boolean;
  onFinished?: (event: JobTelemetryEvent | null) => void;
  onError?: (err: Error) => void;
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
  options: UseJobTelemetryOptions = {}
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

    let isClosed = false;

    function applyEvent(ev: JobTelemetryEvent, terminal = false) {
      setLastEvent(ev);
      if (ev.phase) setPhase(ev.phase);
      if (ev.phaseMessage !== undefined) setPhaseMessage(ev.phaseMessage);
      if (typeof ev.progress === "number") setProgress(ev.progress);
      if (ev.vramUsedGb !== undefined) setVramUsedGb(ev.vramUsedGb);
      if (ev.vramReservedGb !== undefined) setVramReservedGb(ev.vramReservedGb);
      if (ev.stepTimeSeconds !== undefined) setStepTimeSeconds(ev.stepTimeSeconds);
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

      const poll = async () => {
        if (isClosed) return;
        try {
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
            ...(j.vramReservedGb != null ? { vramReservedGb: j.vramReservedGb } : {}),
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

      es.onopen = () => {
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
        // Se SSE falhar, faz fallback transparente para polling HTTP
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
