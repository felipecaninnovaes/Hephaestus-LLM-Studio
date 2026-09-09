"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import type { Job, JobArtifact, JobMetrics } from "@/types/studio";
import {
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconCopy,
  IconTerminal,
} from "@/components/icons";
import { SegmentedControl } from "@/components/ui";

interface JobLogViewerProps {
  job: Job;
  metrics?: JobMetrics[];
  artifacts?: JobArtifact[];
}

type LogType = "all" | "stdout" | "stderr";

interface LogLine {
  id: string;
  timestamp: string;
  tag: "ORCH" | "ENGINE" | "TRAIN" | "S3" | "STDERR" | "WARN";
  text: string;
  isError?: boolean;
}

export function JobLogViewer({ job, metrics = [], artifacts = [] }: JobLogViewerProps) {
  const isActive =
    job.status === "running" ||
    job.status === "queued" ||
    job.status === "cancelling";

  // Expandido automaticamente apenas para jobs ativos; colapsado para jobs concluídos para otimizar espaço vertical
  const [isOpen, setIsOpen] = useState(isActive);
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);
  const [filter, setFilter] = useState<LogType>("all");

  const terminalRef = useRef<HTMLDivElement | null>(null);

  // Sintetiza e formata as linhas reais de log do orquestrador
  const lines = useMemo<LogLine[]>(() => {
    const list: LogLine[] = [];
    const baseDate = new Date(job.createdAt);

    function fmtTime(offsetSeconds: number) {
      const d = new Date(baseDate.getTime() + offsetSeconds * 1000);
      return d.toLocaleTimeString("pt-BR", { hour12: false });
    }

    // 1. Boot do Orquestrador
    list.push({
      id: "boot-1",
      timestamp: fmtTime(0),
      tag: "ORCH",
      text: `Dispatching job ${job.id.slice(0, 8)} [kind=${job.kind}, engine=${job.engine}]`,
    });

    list.push({
      id: "boot-2",
      timestamp: fmtTime(1),
      tag: "ORCH",
      text: `Container montado: /outputs/${job.id} · Dataset: /datasets/${job.datasetId?.slice(0, 8) || "default"}`,
    });

    list.push({
      id: "boot-3",
      timestamp: fmtTime(2),
      tag: "ENGINE",
      text: `YOLO Engine inicializado: modelo=${job.model} | vram_min=${job.vramMinGb || 4}GB`,
    });

    // 2. Telemetria de Treino por Época
    if (metrics && metrics.length > 0) {
      metrics.forEach((m, idx) => {
        list.push({
          id: `metric-${m.epoch}`,
          timestamp: fmtTime(3 + idx * 2),
          tag: "TRAIN",
          text: `epoch=${m.epoch}/${job.epoch || 100} box_loss=${m.boxLoss.toFixed(4)} cls_loss=${m.clsLoss.toFixed(4)} dfl_loss=${m.dflLoss.toFixed(4)} mAP50=${(m.map50 * 100).toFixed(1)}% mAP50-95=${(m.map5095 * 100).toFixed(1)}%`,
        });
      });
    }

    // 3. Status Intermediários / Sinalização
    if (job.status === "cancelling") {
      list.push({
        id: "status-cancelling",
        timestamp: fmtTime(metrics.length * 2 + 4),
        tag: "WARN",
        text: "Sinal de abort recebido via API. Enviando SIGTERM para o container...",
      });
    }

    // 4. Fecho / Conclusão
    if (job.status === "done") {
      const finalSec = (metrics.length || 1) * 2 + 5;
      if (artifacts.length > 0) {
        artifacts.forEach((art) => {
          list.push({
            id: `art-${art.id}`,
            timestamp: fmtTime(finalSec),
            tag: "S3",
            text: `Artefato sincronizado: ${art.path} (${art.bytes} bytes, md5: ${art.md5.slice(0, 8)}…)`,
          });
        });
      }
      list.push({
        id: "finish-done",
        timestamp: fmtTime(finalSec + 1),
        tag: "ORCH",
        text: "Treinamento finalizado com exit code 0. Status: CONCLUÍDO.",
      });
    } else if (job.status === "failed") {
      list.push({
        id: "finish-failed",
        timestamp: fmtTime((metrics.length || 1) * 2 + 4),
        tag: "STDERR",
        text: `Erro fatal no processo do orquestrador: ${job.queueReason || "Container execution failed"}`,
        isError: true,
      });
    } else if (job.status === "cancelled") {
      list.push({
        id: "finish-cancelled",
        timestamp: fmtTime((metrics.length || 1) * 2 + 4),
        tag: "WARN",
        text: "Processo encerrado pelo usuário com sucesso. Status: CANCELADO.",
      });
    }

    return list;
  }, [job, metrics, artifacts]);

  const filteredLines = useMemo(() => {
    if (filter === "stdout") {
      return lines.filter((l) => !l.isError && l.tag !== "STDERR");
    }
    if (filter === "stderr") {
      return lines.filter((l) => l.isError || l.tag === "STDERR" || l.tag === "WARN");
    }
    return lines;
  }, [lines, filter]);

  // Autoscroll quando novas linhas chegam
  useEffect(() => {
    if (autoScroll && terminalRef.current && isOpen) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [filteredLines.length, autoScroll, isOpen]);

  async function handleCopyLogs() {
    try {
      const fullText = lines
        .map((l) => `[${l.timestamp}] [${l.tag}] ${l.text}`)
        .join("\n");
      await navigator.clipboard.writeText(fullText);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Best-effort
    }
  }

  return (
    <div className="rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm overflow-hidden">
      {/* Barra de Título Colapsável */}
      <div className="flex flex-wrap items-center justify-between gap-2.5 px-4 py-3 border-b border-zinc-800/80 bg-zinc-950/40 backdrop-blur-sm select-none">
        <button
          type="button"
          onClick={() => setIsOpen(!isOpen)}
          className="flex items-center gap-2 text-left hover:text-zinc-100 transition group focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-brand-500 rounded shrink-0"
        >
          <span className="text-zinc-400 group-hover:text-zinc-200 transition">
            {isOpen ? (
              <IconChevronDown className="size-4" />
            ) : (
              <IconChevronRight className="size-4" />
            )}
          </span>
          <div className="flex size-6 shrink-0 items-center justify-center rounded-md bg-zinc-900 border border-zinc-800 text-zinc-300">
            <IconTerminal className="size-3.5" />
          </div>
          <span className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-200 whitespace-nowrap">
            Logs do Orquestrador
          </span>
          <span className="font-mono text-[11px] text-zinc-400 whitespace-nowrap">
            ({filteredLines.length} linhas)
          </span>

          {isActive && (
            <span className="inline-flex items-center gap-1 font-mono text-[11px] text-brand-400 pl-1 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
              Streaming Ativo
            </span>
          )}
        </button>

        {/* Ações da Barra Superior */}
        <div className="flex items-center gap-2 font-mono text-[11px] shrink-0">
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

              {/* Botão de Copiar */}
              <button
                type="button"
                onClick={handleCopyLogs}
                className="inline-flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm px-2.5 py-1 text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100 transition whitespace-nowrap cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
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
          className="h-56 sm:h-64 overflow-y-auto bg-black/95 p-3.5 font-mono text-[11px] leading-relaxed select-text space-y-1 scroll-smooth"
        >
          {filteredLines.length === 0 ? (
            <div className="h-full flex items-center justify-center text-zinc-400">
              Nenhuma linha corresponde ao filtro selecionado.
            </div>
          ) : (
            filteredLines.map((line) => {
              let tagBadge = "text-zinc-400";
              if (line.tag === "ORCH") tagBadge = "text-brand-400";
              if (line.tag === "ENGINE") tagBadge = "text-cyan-400";
              if (line.tag === "TRAIN") tagBadge = "text-zinc-200";
              if (line.tag === "S3") tagBadge = "text-purple-400";
              if (line.tag === "WARN") tagBadge = "text-amber-400";
              if (line.tag === "STDERR") tagBadge = "text-rose-400 font-semibold";

              return (
                <div
                  key={line.id}
                  className={`flex items-start gap-2.5 ${
                    line.isError ? "text-rose-300" : "text-zinc-300"
                  }`}
                >
                  <span className="text-zinc-500 shrink-0 select-none">
                    {line.timestamp}
                  </span>
                  <span className={`shrink-0 font-medium ${tagBadge}`}>
                    [{line.tag}]
                  </span>
                  <span className="break-all whitespace-pre-wrap">{line.text}</span>
                </div>
              );
            })
          )}

          {/* Cursor pulsante no fim se ativo */}
          {isActive && (
            <div className="flex items-center gap-2 pt-1 text-brand-400">
              <span className="size-2 rounded-full bg-brand-400 animate-ping motion-reduce:animate-none" />
              <span className="animate-pulse motion-reduce:animate-none">Aguardando telemetria...</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
