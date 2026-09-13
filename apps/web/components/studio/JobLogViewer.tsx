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
  compact?: boolean;
  defaultOpen?: boolean;
}

type LogType = "all" | "stdout" | "stderr";

interface LogLine {
  id: string;
  timestamp: string;
  tag: "ORCH" | "ENGINE" | "TRAIN" | "DIFFUSION" | "AUTOLABEL" | "S3" | "STDERR" | "WARN";
  text: string;
  isError?: boolean;
}

export function JobLogViewer({
  job,
  metrics = [],
  artifacts = [],
  compact = false,
  defaultOpen,
}: JobLogViewerProps) {
  const isActive =
    job.status === "running" ||
    job.status === "queued" ||
    job.status === "cancelling";

  // Expandido por padrão para jobs ativos ou se defaultOpen for fornecido
  const [isOpen, setIsOpen] = useState(defaultOpen !== undefined ? defaultOpen : isActive);
  const [autoScroll, setAutoScroll] = useState(true);
  const [copied, setCopied] = useState(false);
  const [filter, setFilter] = useState<LogType>("all");

  const terminalRef = useRef<HTMLDivElement | null>(null);

  // Sintetiza e formata as linhas reais de log e telemetria do orquestrador
  const lines = useMemo<LogLine[]>(() => {
    const list: LogLine[] = [];
    const baseDate = new Date(job.createdAt);

    function fmtTime(offsetSeconds: number) {
      const d = new Date(baseDate.getTime() + offsetSeconds * 1000);
      return d.toLocaleTimeString("pt-BR", { hour12: false });
    }

    // 1. Boot do Orquestrador e Identificação de Nó
    list.push({
      id: "boot-1",
      timestamp: fmtTime(0),
      tag: "ORCH",
      text: `Dispatching job ${job.id.slice(0, 8)} [kind=${job.kind}, engine=${job.engine}]`,
    });

    const nodeName = job.orchestratorName || (job.orchestratorId ? job.orchestratorId.slice(0, 8) : "Orquestrador Local");
    const nodeKind = job.orchestratorKind ? `[${job.orchestratorKind}]` : "[local]";
    const fallbackNotice = job.orchestratorFallback ? " · Fallback automático ativado" : "";
    list.push({
      id: "boot-node",
      timestamp: fmtTime(1),
      tag: "ORCH",
      text: `Nó Executor: ${nodeName} ${nodeKind}${fallbackNotice}`,
    });

    list.push({
      id: "boot-2",
      timestamp: fmtTime(1),
      tag: "ORCH",
      text: `Container montado: /outputs/${job.id} · Dataset: /datasets/${job.datasetId?.slice(0, 8) || "default"}`,
    });

    // Identificação do Engine específico
    let engineDesc = `YOLO Engine inicializado: modelo=${job.model} | vram_min=${job.vramMinGb || 4}GB`;
    if (job.kind === "autolabel" || job.engine === "autolabel") {
      engineDesc = `AutoLabel Engine inicializado: modelo=${job.model} | Pipeline VLM Vision API`;
    } else if (job.kind === "autotracker" || job.engine === "autotracker") {
      engineDesc = `AutoTracker Engine inicializado: modelo=${job.model} | Open-Vocab Tracking`;
    } else if (job.kind === "diffusion_train" || (job.kind as string) === "diffusion" || job.engine === "diffusion") {
      engineDesc = `Diffusion LoRA Engine inicializado: modelo=${job.model} | vram_min=${job.vramMinGb || 8}GB`;
    } else if (job.kind === "yolo_predict" || job.mode === "predict") {
      engineDesc = `YOLO Predict Engine inicializado: modelo=${job.model}`;
    }

    list.push({
      id: "boot-3",
      timestamp: fmtTime(2),
      tag: "ENGINE",
      text: engineDesc,
    });

    // 2. Telemetria / Progresso
    if (metrics && metrics.length > 0) {
      metrics.forEach((m, idx) => {
        if (job.kind === "autolabel" || job.engine === "autolabel") {
          const totalExpected = job.step || m.step || undefined;
          const prog = m.progress !== undefined && m.progress !== null
            ? `${Math.round(m.progress * 100)}%`
            : totalExpected ? `${Math.round((m.epoch / totalExpected) * 100)}%` : `item ${m.epoch}`;
          list.push({
            id: `metric-${m.epoch}`,
            timestamp: fmtTime(3 + idx * 2),
            tag: "AUTOLABEL",
            text: `Processamento de legendas: item ${m.epoch}${totalExpected ? `/${totalExpected}` : ""} · Progresso: ${prog}`,
          });
        } else if (job.kind === "diffusion_train" || (job.kind as string) === "diffusion" || job.engine === "diffusion" || m.loss !== undefined) {
          const parts: string[] = [`epoch=${m.epoch}/${job.epoch || 100}`];
          if (m.loss !== undefined) parts.push(`loss=${m.loss.toFixed(4)}`);
          if (m.lr !== undefined) parts.push(`lr=${m.lr.toExponential(2)}`);
          if (m.step !== undefined) parts.push(`step=${m.step}`);
          list.push({
            id: `metric-${m.epoch}-${m.step ?? idx}`,
            timestamp: fmtTime(3 + idx * 2),
            tag: "DIFFUSION",
            text: parts.join(" "),
          });
        } else {
          const parts: string[] = [`epoch=${m.epoch}/${job.epoch || 100}`];
          if (m.boxLoss !== undefined) parts.push(`box_loss=${m.boxLoss.toFixed(4)}`);
          if (m.clsLoss !== undefined) parts.push(`cls_loss=${m.clsLoss.toFixed(4)}`);
          if (m.dflLoss !== undefined) parts.push(`dfl_loss=${m.dflLoss.toFixed(4)}`);
          if (m.map50 !== undefined) parts.push(`mAP50=${(m.map50 * 100).toFixed(1)}%`);
          if (m.map5095 !== undefined) parts.push(`mAP50-95=${(m.map5095 * 100).toFixed(1)}%`);
          list.push({
            id: `metric-${m.epoch}`,
            timestamp: fmtTime(3 + idx * 2),
            tag: "TRAIN",
            text: parts.join(" "),
          });
        }
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

      let doneMsg = "Processo finalizado com exit code 0. Status: CONCLUÍDO.";
      if (job.kind === "autolabel" || job.engine === "autolabel") {
        doneMsg = "AutoLabel finalizado com sucesso. Legendas geradas e prontas para aplicação.";
      } else if (job.kind === "autotracker" || job.engine === "autotracker") {
        doneMsg = "AutoTracker finalizado com sucesso. Caixas delimitadoras geradas e prontas para aplicação.";
      } else if ((job.kind as string) === "diffusion" || job.engine === "diffusion") {
        doneMsg = "Treinamento LoRA concluído com sucesso. Adaptador de difusão gerado.";
      } else if (job.kind === "yolo_predict" || job.mode === "predict") {
        doneMsg = "Predição finalizada com sucesso. Detecções exportadas.";
      }

      list.push({
        id: "finish-done",
        timestamp: fmtTime(finalSec + 1),
        tag: "ORCH",
        text: doneMsg,
      });
    } else if (job.status === "failed") {
      const errDetail = job.error || job.queueReason || "Container execution failed";
      const errLines = errDetail.split("\n").map((l) => l.trimEnd()).filter(Boolean);
      list.push({
        id: "finish-failed",
        timestamp: fmtTime((metrics.length || 1) * 2 + 4),
        tag: "STDERR",
        text: `Erro fatal no processo do orquestrador: ${errLines[0] || "Container execution failed"}`,
        isError: true,
      });
      errLines.slice(1).forEach((line, idx) => {
        list.push({
          id: `finish-failed-detail-${idx}`,
          timestamp: fmtTime((metrics.length || 1) * 2 + 4),
          tag: "STDERR",
          text: line,
          isError: true,
        });
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
          <span className="font-mono text-[11px] font-semibold uppercase tracking-caps text-zinc-200 whitespace-nowrap">
            Logs do Orquestrador
          </span>
          <span className="font-mono text-[10px] text-zinc-400 whitespace-nowrap">
            ({filteredLines.length})
          </span>

          {isActive && (
            <span className="inline-flex items-center gap-1 font-mono text-[10px] text-brand-400 pl-1 whitespace-nowrap">
              <span className="size-1.5 rounded-full bg-brand-400 animate-pulse motion-reduce:animate-none" />
              Streaming Ativo
            </span>
          )}
        </button>

        {/* Ações da Barra Superior */}
        <div className="flex items-center gap-1.5 font-mono text-[10px] shrink-0">
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
                className="inline-flex items-center gap-1 rounded-md border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm px-2 py-0.5 text-zinc-300 hover:bg-zinc-800 hover:text-zinc-100 transition whitespace-nowrap cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 text-[10px]"
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
              ? "h-40 sm:h-48 p-2.5 text-[10px] space-y-0.5"
              : "h-56 sm:h-64 p-3.5 text-[11px] space-y-1"
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
                  <span className="text-zinc-600 shrink-0 select-none">
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
            <div className="flex items-center gap-2 pt-1 text-brand-400 text-[10px]">
              <span className="size-1.5 rounded-full bg-brand-400 animate-ping motion-reduce:animate-none" />
              <span className="animate-pulse motion-reduce:animate-none">Aguardando telemetria...</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
