"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Job, JobArtifact, JobMetrics } from "@/types/studio";
import {
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCopy,
  IconTerminal,
} from "@/components/icons";
import { SegmentedControl } from "@/components/ui";
import { copyToClipboard } from "@/lib/clipboard";
import {
  fetchJobLogs,
  formatJobLogTimestamp,
  jobLogLineText,
  type JobLogLine,
} from "@/lib/jobLogs";

interface JobLogViewerProps {
  job: Job;
  metrics?: JobMetrics[];
  artifacts?: JobArtifact[];
  compact?: boolean;
  defaultOpen?: boolean;
  livePhase?: string | null;
  livePhaseMessage?: string | null;
}

type LogType = "all" | "stdout" | "stderr";

interface LogLine {
  id: string;
  timestamp: string;
  tag: "ORCH" | "ENGINE" | "TRAIN" | "DIFFUSION" | "AUTOLABEL" | "S3" | "STDERR" | "WARN";
  text: string;
  isError?: boolean;
  syntheticTime?: boolean;
}

/** Linha persistida do wire (C2a) → LogLine do terminal. */
function toPersistedLogLine(l: JobLogLine, key: string): LogLine {
  const ts = formatJobLogTimestamp(l.timestamp);
  const phase = l.phase ?? "";
  const tag: LogLine["tag"] =
    phase === "error" || phase === "failed"
      ? "STDERR"
      : phase.includes("dataset") ||
          phase.includes("weights") ||
          phase.includes("container") ||
          phase.includes("download") ||
          phase.includes("packaging") ||
          phase.includes("preparing")
        ? "ORCH"
        : phase.includes("sample") || phase === "generating"
          ? "DIFFUSION"
          : phase === "training" || phase.includes("epoch")
            ? "TRAIN"
            : "ENGINE";
  return {
    id: `hist-${key}`,
    timestamp: ts.text,
    syntheticTime: ts.synthetic,
    tag,
    text: jobLogLineText(l),
    isError: tag === "STDERR",
  };
}

export function JobLogViewer({
  job,
  metrics = [],
  artifacts = [],
  compact = false,
  defaultOpen,
  livePhase,
  livePhaseMessage,
}: JobLogViewerProps) {
  const isActive =
    job.status === "running" ||
    job.status === "preparing" ||
    job.status === "queued" ||
    job.status === "dispatched" ||
    job.status === "cancelling";

  // Expandido por padrão para jobs ativos ou se defaultOpen for fornecido
  const [isOpen, setIsOpen] = useState(defaultOpen !== undefined ? defaultOpen : isActive);
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);
  const [filter, setFilter] = useState<LogType>("all");

  const terminalRef = useRef<HTMLDivElement | null>(null);

  // Histórico de transições de telemetria ao vivo durante o ciclo do job
  const [liveLogEntries, setLiveLogEntries] = useState<LogLine[]>([]);
  const lastMsgRef = useRef<string | null>(null);

  useEffect(() => {
    const currentMsg = livePhaseMessage || job.phaseMessage;
    const currentPhase = livePhase || job.phase;
    if (!currentMsg && !currentPhase) return;

    const signature = `${currentPhase || ""}:${currentMsg || ""}`;
    if (signature === lastMsgRef.current) return;
    lastMsgRef.current = signature;

    const tag: LogLine["tag"] =
      currentPhase?.includes("dataset") || currentPhase?.includes("weights") || currentPhase?.includes("container") || currentPhase?.includes("download")
        ? "ORCH"
        : currentPhase?.includes("sample") || currentPhase === "generating"
          ? "DIFFUSION"
          : "ENGINE";

    const text = currentMsg || `Fase: ${currentPhase}`;
    const nowTime = new Date().toLocaleTimeString("pt-BR", { hour12: false });

    setLiveLogEntries((prev) => {
      if (prev.length > 0 && prev[prev.length - 1].text === text) return prev;
      return [
        ...prev,
        {
          id: `live-${Date.now()}-${prev.length}`,
          timestamp: nowTime,
          syntheticTime: false,
          tag,
          text,
        },
      ];
    });
  }, [livePhase, livePhaseMessage, job.phase, job.phaseMessage]);

  // Se o job reiniciar ou trocar, reseta o live log
  const jobId = job.id;
  useEffect(() => {
    if (jobId) {
      setLiveLogEntries([]);
      lastMsgRef.current = null;
    }
  }, [jobId]);

  // C2a: histórico persistido (GET /api/jobs/:id/logs) — sobrevive a refresh.
  const [historyLines, setHistoryLines] = useState<LogLine[]>([]);
  const [historyOffset, setHistoryOffset] = useState(0);
  const [historyEof, setHistoryEof] = useState(true);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyFailed, setHistoryFailed] = useState(false);
  const historyLoadedFor = useRef<string | null>(null);

  const loadHistory = useCallback(
    async (offset: number) => {
      if (!jobId || historyLoading) return;
      setHistoryLoading(true);
      try {
        const page = await fetchJobLogs(jobId, offset);
        const mapped = page.lines
          .map((l, i) => toPersistedLogLine(l, `${offset + i}`))
          .filter((l) => l.text.length > 0);
        setHistoryLines((prev) => (offset === 0 ? mapped : [...prev, ...mapped]));
        setHistoryOffset(page.nextOffset);
        setHistoryEof(page.eof);
        setHistoryFailed(false);
      } catch {
        // Endpoint ainda não deployado (janela segura) ou BFF fora: o viewer
        // segue com as linhas sintetizadas de sempre — nunca quebra a tela.
        setHistoryFailed(true);
      } finally {
        setHistoryLoading(false);
      }
    },
    [jobId, historyLoading],
  );

  // Primeira página quando o painel abre (e apenas uma vez por job).
  useEffect(() => {
    if (!jobId || !isOpen || historyLoadedFor.current === jobId) return;
    historyLoadedFor.current = jobId;
    void loadHistory(0);
  }, [jobId, isOpen, loadHistory]);

  // Troca de job: zera o histórico para não vazar linhas do job anterior.
  useEffect(() => {
    if (!jobId) return;
    setHistoryLines([]);
    setHistoryOffset(0);
    setHistoryEof(true);
    setHistoryFailed(false);
    historyLoadedFor.current = null;
  }, [jobId]);
  // Sintetiza e formata as linhas reais de log e telemetria do orquestrador
  const lines = useMemo<LogLine[]>(() => {
    const list: LogLine[] = [];

    // 1. Boot — única linha honesta derivada dos dados reais do job
    const nodeName = job.orchestratorName || (job.orchestratorId ? job.orchestratorId.slice(0, 8) : "—");
    list.push({
      id: "boot",
      timestamp: "—",
      syntheticTime: true,
      tag: "ORCH",
      text: `Job submetido · kind=${job.kind} · engine=${job.engine ?? "—"} · nó=${nodeName}`,
    });

    // 1.5 C2a: histórico persistido do jsonl do engine, dedupe por texto com
    //     o que já foi sintetizado. Só quando existe — jobs antigos ou janela
    //     pré-deploy caem no comportamento sintetizado de sempre.
    if (historyLines.length > 0) {
      const seen = new Set(list.map((l) => l.text));
      for (const h of historyLines) {
        if (!seen.has(h.text)) {
          list.push(h);
          seen.add(h.text);
        }
      }
    }

    // 2. Telemetria / Progresso (suprimido quando o log persistido já cobre)
    if (metrics && metrics.length > 0 && historyLines.length === 0) {
      metrics.forEach((m, idx) => {
        if (job.kind === "autolabel" || job.engine === "autolabel") {
          const totalExpected = job.step || m.step || undefined;
          const prog = m.progress !== undefined && m.progress !== null
            ? `${Math.round(m.progress * 100)}%`
            : totalExpected ? `${Math.round((m.epoch / totalExpected) * 100)}%` : `item ${m.epoch}`;
          list.push({
            id: `metric-${m.epoch}`,
            timestamp: "—",
            syntheticTime: true,
            tag: "AUTOLABEL",
            text: `Processamento de legendas: item ${m.epoch}${totalExpected ? `/${totalExpected}` : ""} · Progresso: ${prog}`,
          });
        } else if (job.kind === "diffusion_train" || (job.kind as string) === "diffusion" || job.engine === "diffusion" || m.loss !== undefined) {
          if (m.message || m.phase) {
            list.push({
              id: `metric-phase-${m.epoch}-${m.step ?? idx}`,
              timestamp: "—",
              syntheticTime: true,
              tag: "DIFFUSION",
              text: m.message || `Fase: ${m.phase}`,
            });
          } else {
            const parts: string[] = [`epoch=${m.epoch}/${job.epoch || 100}`];
            if (m.loss !== undefined) parts.push(`loss=${m.loss.toFixed(4)}`);
            if (m.lr !== undefined) parts.push(`lr=${m.lr.toExponential(2)}`);
            if (m.step !== undefined) parts.push(`step=${m.step}`);
            list.push({
              id: `metric-${m.epoch}-${m.step ?? idx}`,
              timestamp: "—",
              syntheticTime: true,
              tag: "DIFFUSION",
              text: parts.join(" "),
            });
          }
        } else {
          const parts: string[] = [`epoch=${m.epoch}/${job.epoch || 100}`];
          if (m.boxLoss !== undefined) parts.push(`box_loss=${m.boxLoss.toFixed(4)}`);
          if (m.clsLoss !== undefined) parts.push(`cls_loss=${m.clsLoss.toFixed(4)}`);
          if (m.dflLoss !== undefined) parts.push(`dfl_loss=${m.dflLoss.toFixed(4)}`);
          if (m.map50 !== undefined) parts.push(`mAP50=${(m.map50 * 100).toFixed(1)}%`);
          if (m.map5095 !== undefined) parts.push(`mAP50-95=${(m.map5095 * 100).toFixed(1)}%`);
          list.push({
            id: `metric-${m.epoch}`,
            timestamp: "—",
            syntheticTime: true,
            tag: "TRAIN",
            text: parts.join(" "),
          });
        }
      });
    }
    // 2.5 Eventos de telemetria ao vivo (download, staging, passos de amostras)
    if (liveLogEntries.length > 0) {
      liveLogEntries.forEach((entry) => {
        if (!list.some((l) => l.text === entry.text)) {
          list.push(entry);
        }
      });
    }

    // 3. Status Intermediários / Sinalização
    if (job.status === "cancelling") {
      list.push({
        id: "status-cancelling",
        timestamp: "—",
        syntheticTime: true,
        tag: "WARN",
        text: "Cancelamento solicitado via API. Aguardando encerramento da execução no nó...",
      });
    }

    // 4. Fecho / Conclusão
    if (job.status === "done") {
      if (artifacts.length > 0) {
        artifacts.forEach((art) => {
          list.push({
            id: `art-${art.id}`,
            timestamp: "—",
            syntheticTime: true,
            tag: "S3",
            text: `Artefato sincronizado: ${art.path} (${art.bytes} bytes, md5: ${art.md5.slice(0, 8)}…)`,
          });
        });
      }

      let doneMsg = "Execução concluída no nó. Status: CONCLUÍDO.";
      if (job.kind === "autolabel" || job.engine === "autolabel") {
        doneMsg = "AutoLabel finalizado com sucesso. Legendas geradas e prontas para aplicação.";
      } else if (job.kind === "autotracker" || job.engine === "autotracker") {
        doneMsg = "AutoTracker finalizado com sucesso. Caixas delimitadoras geradas e prontas para aplicação.";
      } else if ((job.kind as string) === "diffusion" || job.engine === "diffusion") {
        doneMsg = "Treinamento LoRA concluído com sucesso. Adaptador de difusão gerado.";
      } else if (job.kind === "yolo_predict" || job.mode === "predict") {
        doneMsg = "Predição finalizada com sucesso. Detecções exportadas.";
      }

      const finishedTime = job.finishedAt
        ? new Date(job.finishedAt).toLocaleTimeString("pt-BR", { hour12: false })
        : "—";
      list.push({
        id: "finish-done",
        timestamp: finishedTime,
        syntheticTime: !job.finishedAt,
        tag: "ORCH",
        text: doneMsg,
      });
    } else if (job.status === "failed") {
      const errDetail = job.error || job.queueReason || "Container execution failed";
      const errLines = errDetail.split("\n").map((l) => l.trimEnd()).filter(Boolean);
      const finishedTime = job.finishedAt
        ? new Date(job.finishedAt).toLocaleTimeString("pt-BR", { hour12: false })
        : "—";
      list.push({
        id: "finish-failed",
        timestamp: finishedTime,
        syntheticTime: !job.finishedAt,
        tag: "STDERR",
        text: `Erro reportado: ${errLines[0] || "Container execution failed"}`,
        isError: true,
      });
      errLines.slice(1).forEach((line, idx) => {
        list.push({
          id: `finish-failed-detail-${idx}`,
          timestamp: finishedTime,
          syntheticTime: !job.finishedAt,
          tag: "STDERR",
          text: line,
          isError: true,
        });
      });
    } else if (job.status === "cancelled") {
      const finishedTime = job.finishedAt
        ? new Date(job.finishedAt).toLocaleTimeString("pt-BR", { hour12: false })
        : "—";
      list.push({
        id: "finish-cancelled",
        timestamp: finishedTime,
        syntheticTime: !job.finishedAt,
        tag: "WARN",
        text: "Status: CANCELADO.",
      });
    }

    return list;
  }, [job, metrics, artifacts, liveLogEntries, historyLines]);

  const filteredLines = useMemo(() => {
    if (filter === "stdout") {
      return lines.filter((l) => !l.isError && l.tag !== "STDERR");
    }
    if (filter === "stderr") {
      return lines.filter((l) => l.isError || l.tag === "STDERR" || l.tag === "WARN");
    }
    return lines;
  }, [lines, filter]);

  // Autoscroll quando novas linhas chegam (length lido para reagir a novas linhas;
  // usa o array como dep — scroll é idempotente e barato)
  useEffect(() => {
    const count = filteredLines.length;
    if (count > 0 && autoScroll && terminalRef.current && isOpen) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [filteredLines, autoScroll, isOpen]);

  async function handleCopyLogs() {
    try {
      const fullText = lines
        .map((l) => `${l.syntheticTime ? "" : `[${l.timestamp}] `}[${l.tag}] ${l.text}`)
        .join("\n");
      await copyToClipboard(fullText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Best-effort
    }
  }

  return (
    <div
      className={
        compact
          ? "rounded-lg border border-white/10 bg-black/60 backdrop-blur-sm overflow-hidden"
          : "rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm overflow-hidden"
      }
    >
      {/* Barra de Título Colapsável */}
      <div
        className={`flex flex-wrap items-center justify-between gap-2 border-b bg-zinc-950/40 backdrop-blur-sm select-none ${
          compact ? "px-3 py-2 border-white/10" : "px-4 py-3 border-zinc-800/80"
        }`}
      >
        <button
          type="button"
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-2 text-left hover:text-zinc-100 transition group focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-brand-500 rounded shrink-0 cursor-pointer"
        >
          <span className="text-zinc-400 group-hover:text-zinc-200 transition">
            {isOpen ? (
              <IconChevronDown className={compact ? "size-3.5" : "size-4"} />
            ) : (
              <IconChevronRight className={compact ? "size-3.5" : "size-4"} />
            )}
          </span>
          <div
            className={`flex shrink-0 items-center justify-center rounded-md bg-zinc-900 border border-zinc-800 text-zinc-300 ${
              compact ? "size-5" : "size-6"
            }`}
          >
            <IconTerminal className={compact ? "size-3" : "size-3.5"} />
          </div>
          <span className="font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-200 whitespace-nowrap">
            Logs do Orquestrador
          </span>
          <span className="font-mono text-3xs text-zinc-400 whitespace-nowrap">
            ({filteredLines.length})
          </span>

          {isActive && (
            <span className="inline-flex items-center gap-1 font-mono text-3xs text-brand-400 pl-1 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
              Streaming Ativo
            </span>
          )}
        </button>

        {/* Ações da Barra Superior */}
        <div className="flex items-center gap-1.5 font-mono text-3xs shrink-0">
          {isOpen && (
            <>
              {/* Filtros */}
              <SegmentedControl<LogType>
                value={filter}
                onChange={setFilter}
                options={[
                  { id: "all", label: "Todos" },
                  { id: "stdout", label: "Stdout" },
                  { id: "stderr", label: "Stderr" },
                ]}
              />

              {/* Botão de Auto-scroll */}
              {!compact && (
                <button
                  type="button"
                  onClick={() => setAutoScroll(!autoScroll)}
                  className={`hidden lg:inline-flex items-center gap-1 rounded-md border px-2 py-1 transition whitespace-nowrap backdrop-blur-sm cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
                    autoScroll
                      ? "border-brand-500/30 bg-brand-500/10 text-brand-300"
                      : "border-zinc-800 bg-zinc-900/50 text-zinc-400 hover:text-zinc-300"
                  }`}
                  title="Rolar automaticamente para a última linha"
                >
                  Auto-scroll: {autoScroll ? "ON" : "OFF"}
                </button>
              )}

              {/* Botão de Copiar */}
              <button
                type="button"
                onClick={handleCopyLogs}
                className="inline-flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm px-2 py-0.5 text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100 transition whitespace-nowrap cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 text-3xs"
              >
                {copied ? (
                  <>
                    <IconCheck className="size-3 text-brand-400" />
                    <span className="text-brand-400">Copiado</span>
                  </>
                ) : (
                  <>
                    <IconCopy className="size-3" />
                    <span>Copiar</span>
                  </>
                )}
              </button>
            </>
          )}
        </div>
      </div>

      {/* Corpo do Terminal (Expandido) */}
      {isOpen && (
        <div
          ref={terminalRef}
          className={`overflow-y-auto bg-black/95 font-mono leading-relaxed select-text scroll-smooth ${
            compact
              ? "h-40 sm:h-48 p-2.5 text-3xs space-y-0.5"
              : "h-56 sm:h-64 p-3.5 text-2xs space-y-1"
          }`}
        >
          {filteredLines.length === 0 ? (
            <div className="h-full flex items-center justify-center text-zinc-500 text-xs">
              Nenhuma linha corresponde ao filtro selecionado.
            </div>
          ) : (
            filteredLines.map((line) => {
              let tagBadge = "text-zinc-400";
              if (line.tag === "ORCH") tagBadge = "text-brand-400";
              if (line.tag === "ENGINE") tagBadge = "text-cyan-400";
              if (line.tag === "TRAIN") tagBadge = "text-zinc-200";
              if (line.tag === "DIFFUSION") tagBadge = "text-indigo-400";
              if (line.tag === "AUTOLABEL") tagBadge = "text-sky-400";
              if (line.tag === "S3") tagBadge = "text-purple-400";
              if (line.tag === "WARN") tagBadge = "text-amber-400";
              if (line.tag === "STDERR") tagBadge = "text-rose-400 font-semibold";

              return (
                <div
                  key={line.id}
                  className={`flex items-start gap-2 ${
                    line.isError ? "text-rose-300" : "text-zinc-300"
                  }`}
                >
                  <span
                    className={`shrink-0 select-none ${line.syntheticTime ? "text-zinc-700" : "text-zinc-600"}`}
                    title={line.syntheticTime ? "Hora não registrada pelo orquestrador" : undefined}
                  >
                    {line.syntheticTime ? "—" : line.timestamp}
                  </span>
                  <span className={`shrink-0 font-medium ${tagBadge}`}>
                    [{line.tag}]
                  </span>
                  <span className="break-all whitespace-pre-wrap">{line.text}</span>
                </div>
              );
            })
          )}

          {/* C2a: paginação do histórico persistido */}
          {historyFailed && (
            <div className="pt-1 text-amber-400/80 text-3xs">
              Histórico persistido indisponível — exibindo eventos desta sessão.
            </div>
          )}
          {!historyFailed && !historyEof && (
            <button
              type="button"
              onClick={() => void loadHistory(historyOffset)}
              disabled={historyLoading}
              className="mt-1 inline-flex items-center gap-1.5 rounded border border-zinc-800 bg-zinc-900/60 px-2 py-1 font-mono text-3xs text-zinc-400 hover:text-zinc-200 transition disabled:opacity-50 cursor-pointer"
            >
              {historyLoading ? "Carregando…" : "Carregar mais linhas"}
            </button>
          )}

          {/* Cursor pulsante no fim se ativo */}
          {isActive && (
            <div className="flex items-center gap-2 pt-1 text-brand-400 text-3xs">
              <span className="size-1.5 rounded-full bg-brand-400 animate-ping motion-reduce:animate-none" />
              <span className="animate-pulse motion-reduce:animate-none">Aguardando telemetria...</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
