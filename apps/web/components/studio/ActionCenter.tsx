"use client";

import React, { useEffect, useMemo, useState } from "react";
import {
  IconCheck,
  IconChevronDown,
  IconRefresh,
  IconSearch,
  IconX,
  IconZap,
} from "@/components/icons";

export type ActivityStatus = "success" | "failed" | "running" | "warning";

export interface ActivityItem {
  id: string;
  title: string;
  timestamp: string;
  description: string;
  origin: string;
  status: ActivityStatus;
  statusLabel: string;
  details?: string;
}

interface ActionCenterProps {
  open: boolean;
  onClose: () => void;
  activities?: ActivityItem[];
}

const DEFAULT_ACTIVITIES: ActivityItem[] = [
  {
    id: "act-1",
    title: "51 imagens verificadas",
    timestamp: "há 30 minutos",
    description: "Verificação de atualização de imagem",
    origin: "Local Docker · Iniciado por System",
    status: "failed",
    statusLabel: "Falhou",
    details: "Image update check completed: 46 checked, 5 errors. Timeout ao sincronizar com registry remoto.",
  },
  {
    id: "act-2",
    title: "Treino YOLO v11m",
    timestamp: "há 2 horas",
    description: "Epoch 84/100 · loss 0.0241 · mAP50 0.942",
    origin: "Pod RunPod A100 · Iniciado por Felipe",
    status: "running",
    statusLabel: "Em execução",
    details: "Runner ativo com 18.2 GB de VRAM alocada. Próximo checkpoint em 5 epochs.",
  },
  {
    id: "act-3",
    title: "ec1950b5e543f04dec69e2febac89d2df40677d758c0f45f24bc811eff33b138",
    timestamp: "há 5 horas",
    description: "Início do contêiner PyTorch Studio",
    origin: "Local Docker · Iniciado por Arcane Admin",
    status: "success",
    statusLabel: "Sucesso",
    details: "Container started. Daemon inicializado com portas 8080 e 3000 vinculadas.",
  },
  {
    id: "act-4",
    title: "f55ea506d2ae6bf407ea784c5020573ab726c3d3e92a0e662fba3b841a262481",
    timestamp: "há 5 horas",
    description: "Parada de contêiner efêmero de validação",
    origin: "Local Docker · Iniciado por Arcane Admin",
    status: "success",
    statusLabel: "Sucesso",
    details: "Container stopped cleanly with exit code 0.",
  },
  {
    id: "act-5",
    title: "Sincronização de Embeddings CLIP",
    timestamp: "há 8 horas",
    description: "Indexação semântica de 1.240 amostras",
    origin: "Local Node · AutoTracker",
    status: "success",
    statusLabel: "Sucesso",
    details: "1.240 vetores normalizados e persistidos na base vetorial local.",
  },
];

const STATUS_CONFIG: Record<
  ActivityStatus,
  {
    borderClass: string;
    badgeClass: string;
    iconBg: string;
    iconColor: string;
  }
> = {
  success: {
    borderClass: "bg-[#34d399]",
    badgeClass: "bg-[#34d399]/10 text-[#34d399] border-[#34d399]/30",
    iconBg: "bg-[#34d399]/10",
    iconColor: "text-[#34d399]",
  },
  failed: {
    borderClass: "bg-rose-500",
    badgeClass: "bg-rose-500/10 text-rose-300 border-rose-500/30",
    iconBg: "bg-rose-500/10",
    iconColor: "text-rose-400",
  },
  running: {
    borderClass: "bg-brand-500",
    badgeClass: "bg-brand-500/15 text-brand-300 border-brand-500/35",
    iconBg: "bg-brand-500/15",
    iconColor: "text-brand-400",
  },
  warning: {
    borderClass: "bg-amber-500",
    badgeClass: "bg-amber-400/10 text-amber-300 border-amber-400/30",
    iconBg: "bg-amber-400/10",
    iconColor: "text-amber-400",
  },
};

export function ActionCenter({
  open,
  onClose,
  activities = DEFAULT_ACTIVITIES,
}: ActionCenterProps) {
  const [query, setQuery] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (open) {
      setMounted(true);
      const raf = requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          setVisible(true);
        });
      });
      return () => cancelAnimationFrame(raf);
    } else {
      setVisible(false);
      const timer = setTimeout(() => {
        setMounted(false);
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Bloqueia scroll do fundo durante exibição do drawer
  useEffect(() => {
    if (mounted) {
      const original = document.body.style.overflow;
      document.body.style.overflow = "hidden";
      return () => {
        document.body.style.overflow = original;
      };
    }
  }, [mounted]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return activities;
    return activities.filter(
      (a) =>
        a.title.toLowerCase().includes(q) ||
        a.description.toLowerCase().includes(q) ||
        a.origin.toLowerCase().includes(q) ||
        a.statusLabel.toLowerCase().includes(q),
    );
  }, [activities, query]);

  function toggleExpand(id: string) {
    setExpandedId((prev) => (prev === id ? null : id));
  }

  if (!mounted) return null;

  return (
    <div className="fixed inset-0 z-50 overflow-hidden pointer-events-none">
      {/* Backdrop com desfoque e fade-in fluido */}
      <div
        className={`fixed inset-0 bg-black/60 backdrop-blur-sm transition-opacity duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] pointer-events-auto ${
          visible ? "opacity-100" : "opacity-0"
        }`}
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Drawer deslizante a partir da borda direita com desaceleração cúbica */}
      <aside
        role="dialog"
        aria-modal="true"
        aria-label="Centro de Atividades"
        className={`fixed inset-y-0 right-0 flex w-full flex-col border-l border-white/10 bg-[rgba(18,15,24,0.96)] text-zinc-100 shadow-[-24px_0_60px_rgba(0,0,0,0.85)] backdrop-blur-2xl transition-transform duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] sm:w-[480px] pointer-events-auto ${
          visible ? "translate-x-0" : "translate-x-full"
        }`}
      >
        {/* Hairline zenital com gradiente violeta no topo */}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute top-0 left-0 right-0 h-px"
          style={{
            background:
              "linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)",
          }}
        />

        {/* Header do Action Center */}
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-5">
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
              <IconZap className="size-4" />
            </span>
            <h2 className="font-display text-sm font-bold text-white tracking-tight">
              Centro de Atividades
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Fechar Centro de Atividades"
            className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer"
          >
            <IconX className="size-4" />
          </button>
        </div>

        {/* Barra de Pesquisa */}
        <div className="border-b border-white/10 p-3.5">
          <div className="relative flex items-center rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 transition focus-within:border-brand-500/60 focus-within:ring-1 focus-within:ring-brand-500/30">
            <IconSearch className="size-4 shrink-0 text-zinc-500 mr-2" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Pesquisar atividade…"
              aria-label="Pesquisar atividade"
              className="min-w-0 flex-1 bg-transparent font-sans text-xs text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery("")}
                aria-label="Limpar pesquisa"
                className="text-zinc-500 hover:text-zinc-200 text-xs ml-1"
              >
                ✕
              </button>
            )}
          </div>
        </div>

        {/* Seção de Conteúdo da Timeline */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          <div className="flex items-center justify-between px-1">
            <span className="tracking-caps font-mono text-[10px] font-semibold uppercase text-zinc-400">
              Histórico ({filtered.length})
            </span>
            <span className="font-mono text-[10px] text-zinc-500">
              Stream ao vivo
            </span>
          </div>

          {filtered.length === 0 ? (
            <div className="glass-card rounded-2xl p-8 text-center mt-6">
              <p className="text-xs text-zinc-400">Nenhuma atividade encontrada para &quot;{query}&quot;</p>
            </div>
          ) : (
            <div className="space-y-2.5">
              {filtered.map((item) => {
                const config = STATUS_CONFIG[item.status];
                const isExpanded = expandedId === item.id;

                return (
                  <div
                    key={item.id}
                    className="group relative overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-900/60 transition-all hover:border-zinc-700 hover:bg-zinc-900/90"
                  >
                    {/* Linha vertical indicadora de status na extrema esquerda (estilo Arcane) */}
                    <div
                      className={`absolute top-0 bottom-0 left-0 w-1 ${config.borderClass}`}
                      aria-hidden="true"
                    />

                    <div
                      className="p-3 pl-4 cursor-pointer"
                      onClick={() => toggleExpand(item.id)}
                    >
                      <div className="flex items-start justify-between gap-2.5">
                        <div className="flex items-start space-x-2.5 min-w-0 flex-1">
                          {/* Ícone de atividade */}
                          <div
                            className={`mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full ${config.iconBg} ${config.iconColor}`}
                          >
                            {item.status === "running" ? (
                              <IconRefresh className="size-3.5 animate-spin" />
                            ) : item.status === "success" ? (
                              <IconCheck className="size-3.5" />
                            ) : (
                              <IconRefresh className="size-3.5" />
                            )}
                          </div>

                          <div className="min-w-0 flex-1">
                            <div className="flex items-baseline gap-1.5 flex-wrap">
                              <span className="text-xs font-semibold text-zinc-200 truncate">
                                {item.title}
                              </span>
                              <span className="font-mono text-[10px] text-zinc-500 shrink-0">
                                · {item.timestamp}
                              </span>
                            </div>
                            <p className="mt-0.5 text-[11px] text-zinc-400 line-clamp-1">
                              {item.description}
                            </p>
                            <p className="mt-0.5 font-mono text-[10px] text-zinc-500 truncate">
                              {item.origin}
                            </p>
                          </div>
                        </div>

                        {/* Status Badge + Chevron */}
                        <div className="flex items-center space-x-1.5 shrink-0">
                          <span
                            className={`rounded-full border px-2 py-0.5 font-mono text-[10px] font-medium uppercase tracking-caps ${config.badgeClass}`}
                          >
                            {item.statusLabel}
                          </span>
                          <span
                            className={`text-zinc-500 transition-transform duration-200 ${
                              isExpanded ? "rotate-180" : ""
                            }`}
                          >
                            <IconChevronDown className="size-3.5" />
                          </span>
                        </div>
                      </div>

                      {/* Painel expansível (Accordion de Logs e Detalhes) */}
                      {isExpanded && item.details && (
                        <div className="mt-3 border-t border-zinc-800/80 pt-2.5 text-[11px] font-mono leading-relaxed text-zinc-300 bg-black/30 -mx-3 -mb-3 p-3">
                          <div className="mb-1 text-[10px] text-zinc-500 uppercase tracking-caps">
                            Detalhes da Operação
                          </div>
                          <div className="rounded bg-black/40 p-2 text-zinc-300 border border-zinc-800/60 font-mono text-[10px] whitespace-pre-wrap">
                            {item.details}
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}

export default ActionCenter;
