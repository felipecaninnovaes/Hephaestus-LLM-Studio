"use client";

import { useEffect, useRef, useState } from "react";
import { getJob } from "@/lib/jobs";
import type { Job, JobTelemetryEvent } from "@/types/studio";

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

    let isClosed = false;

    function applyEvent(ev: JobTelemetryEvent, terminal = false) {
      setLastEvent(ev);
      if (ev.phase) setPhase(ev.phase);
      if (ev.phaseMessage !== undefined) setPhaseMessage(ev.phaseMessage);
      if (typeof ev.progress === "number") setProgress(ev.progress);
      if (ev.vramUsedGb !== undefined) setVramUsedGb(ev.vramUsedGb);
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
          const ev: JobTelemetryEvent = {
            timestamp: new Date().toISOString(),
            phase: j.phase || j.status,
            phaseMessage: j.phaseMessage,
            progress: j.progress || 0,
            step: j.step,
            epoch: j.epoch,
            vramUsedGb: j.vramUsedGb,
          };
          const isTerm = ["done", "failed", "cancelled"].includes(j.status);
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
