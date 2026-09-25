"use client";

import { apiFetch } from "@/lib/api";

/**
 * Linha de log persistida de um job (C2a).
 * Espelha o wire `JobLogLine` do BFF em camelCase:
 * linhas malformadas do jsonl chegam com `message` = linha bruta
 * e demais campos nulos.
 */
export interface JobLogLine {
  timestamp: string | null;
  phase: string | null;
  message: string | null;
  progress: number | null;
  epoch: number | null;
  step: number | null;
}

/** Página de logs persistidos (`GET /api/jobs/:id/logs`). */
export interface JobLogPage {
  lines: JobLogLine[];
  nextOffset: number;
  eof: boolean;
}

export const JOB_LOGS_DEFAULT_LIMIT = 500;
export const JOB_LOGS_MAX_LIMIT = 2000;

/**
 * GET /api/jobs/:id/logs?offset=&limit= — histórico persistido de logs do job.
 * `offset` = nº de linhas RAW jsonl já consumidas (default 0).
 * Job sem artefato de log → 200 com `lines: []`, `eof: true` (não 404).
 */
export function fetchJobLogs(
  jobId: string,
  offset = 0,
  limit: number = JOB_LOGS_DEFAULT_LIMIT,
): Promise<JobLogPage> {
  const safeOffset = Math.max(0, Math.floor(offset));
  const safeLimit = Math.min(
    JOB_LOGS_MAX_LIMIT,
    Math.max(1, Math.floor(limit)),
  );
  return apiFetch<JobLogPage>(
    `/api/jobs/${jobId}/logs?offset=${safeOffset}&limit=${safeLimit}`,
  );
}

/**
 * Chave de dedupe SSE×histórico: mesma emissão (ao vivo e persistida)
 * compartilha timestamp+message.
 */
export function jobLogDedupeKey(
  timestamp: string | null | undefined,
  message: string | null | undefined,
): string {
  return `${timestamp ?? ""}::${message ?? ""}`;
}

/** Texto exibível de uma linha de log (message com fallback para phase). */
export function jobLogLineText(line: JobLogLine): string {
  if (line.message) return line.message;
  if (line.phase) return `Fase: ${line.phase}`;
  return "";
}

/**
 * Formata o timestamp do wire para exibição no terminal.
 * ISO válido → hora local pt-BR (mesmo formato das linhas SSE ao vivo,
 * permitindo o dedupe por timestamp+message); ausente/inválido → "—".
 */
export function formatJobLogTimestamp(timestamp: string | null | undefined): {
  text: string;
  synthetic: boolean;
} {
  if (!timestamp) return { text: "—", synthetic: true };
  const time = new Date(timestamp).getTime();
  if (Number.isNaN(time)) return { text: timestamp, synthetic: false };
  return {
    text: new Date(timestamp).toLocaleTimeString("pt-BR", { hour12: false }),
    synthetic: false,
  };
}
