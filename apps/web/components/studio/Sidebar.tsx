"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import Link from "next/link";
import {
  IconActivity,
  IconBox,
  IconChevronRight,
  IconDatabase,
  IconHardDrive,
  IconHome,
  IconImage,
  IconList,
  IconLogOut,
  IconNetwork,
  IconPin,
  IconPlay,
  IconServer,
  IconSettings,
  IconShield,
  IconSparkles,
  IconTarget,
  IconX,
} from "@/components/icons";
import { getTelemetry } from "@/lib/jobs";
import { listOrchestrators, getHealth, type Orchestrator } from "@/lib/monitoring";
import type { Telemetry } from "@/types/studio";

interface SidebarProps {
  open: boolean;
  onClose: () => void;
  onOpenActionCenter?: () => void;
  pinned?: boolean;
  onTogglePin?: () => void;
}

interface NavItem {
  id: string;
  label: string;
  href: string;
  icon: React.ComponentType<{ className?: string }>;
  badge?: string | number;
  hasSubmenu?: boolean;
  isAvailable?: boolean;
}

interface NavSection {
  title: string;
  items: NavItem[];
}

export default function Sidebar({
  open,
  onClose,
  onOpenActionCenter,
  pinned = false,
  onTogglePin,
}: SidebarProps) {
  const router = useRouter();
  const pathname = usePathname();
  const [leaving, setLeaving] = useState(false);
  const [isHovered, setIsHovered] = useState(false);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [productVersion, setProductVersion] = useState<string | null>(null);
  const [orchestrators, setOrchestrators] = useState<Orchestrator[]>([]);
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [confirmingLogout, setConfirmingLogout] = useState(false);
  const userMenuRef = useRef<HTMLDivElement>(null);

  // Fecha o menu de perfil ao trocar de rota ou fechar o drawer
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset intencional on-change — re-executa em pathname/open sem ler valores; reabriria condições de corrida ler estado do menu aqui
  useEffect(() => {
    setUserMenuOpen(false);
    setConfirmingLogout(false);
  }, [pathname, open]);

  // Fecha o menu suspenso do usuário ao clicar fora ou teclar Escape
  useEffect(() => {
    if (!userMenuOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (userMenuRef.current && !userMenuRef.current.contains(e.target as Node)) {
        setUserMenuOpen(false);
        setConfirmingLogout(false);
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setUserMenuOpen(false);
        setConfirmingLogout(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [userMenuOpen]);

  // No mobile, a gaveta quando aberta é SEMPRE expandida com texto e categorias completas.
  // No desktop (≥ 1024px), expande com hover ou se estiver fixada (pinned).
  const isDesktopExpanded = isHovered || pinned;
  const isExpanded = open || isDesktopExpanded;

  // Fecha o drawer mobile ao redimensionar para tela desktop (≥ 1024px)
  useEffect(() => {
    const handleResize = () => {
      if (typeof window !== "undefined" && window.innerWidth >= 1024 && open) {
        onClose();
      }
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, [open, onClose]);

  // Trava o scroll da página de fundo enquanto a gaveta mobile estiver aberta
  useEffect(() => {
    if (open && typeof window !== "undefined" && window.innerWidth < 1024) {
      const prevOverflow = document.body.style.overflow;
      document.body.style.overflow = "hidden";
      return () => {
        document.body.style.overflow = prevOverflow;
      };
    }
  }, [open]);

  useEffect(() => {
    let active = true;
    const fetchTelem = async () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      try {
        const data = await getTelemetry();
        if (active) setTelemetry(data);
      } catch {}
    };
    fetchTelem();
    const interval = setInterval(fetchTelem, 3000);
    const handleVisibilityChange = () => {
      if (typeof document !== "undefined" && document.visibilityState === "visible") {
        fetchTelem();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      active = false;
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  // Busca versão de produto UMA VEZ no mount (rota pública /health)
  useEffect(() => {
    getHealth()
      .then((h) => setProductVersion(h.version))
      .catch(() => {});
  }, []);

  // Busca lista de orquestradores no mesmo tick da telemetria (rotas leves)
  useEffect(() => {
    let active = true;
    const fetchOrchs = async () => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      try {
        const data = await listOrchestrators();
        if (active) setOrchestrators(data.items);
      } catch {}
    };
    fetchOrchs();
    const interval = setInterval(fetchOrchs, 15000);
    const handleVisibilityChange = () => {
      if (typeof document !== "undefined" && document.visibilityState === "visible") {
        fetchOrchs();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      active = false;
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  const sections: NavSection[] = [
    {
      title: "Estúdio & Dados",
      items: [
        {
          id: "dashboard",
          label: "Painel",
          href: "/dashboard",
          icon: IconHome,
          isAvailable: true,
        },
        {
          id: "datasets",
          label: "Datasets",
          href: "/datasets",
          icon: IconDatabase,
          isAvailable: true,
        },
      ],
    },
    {
      title: "Treinamento & Execução",
      items: [
        {
          id: "treino",
          label: "Treino YOLO",
          href: "/treino",
          icon: IconTarget,
          isAvailable: true,
        },
        {
          id: "jobs",
          label: "Execuções",
          href: "/jobs",
          icon: IconActivity,
          badge: telemetry?.jobsActive ? `${telemetry.jobsActive}` : undefined,
          isAvailable: true,
        },
        {
          id: "difusao",
          label: "Difusão LoRA",
          href: "/difusao",
          icon: IconImage,
          isAvailable: true,
        },
        {
          id: "openclip",
          label: "OpenCLIP",
          href: "/openclip",
          icon: IconNetwork,
          badge: "Roadmap",
          isAvailable: false,
        },
        {
          id: "geracao",
          label: "Geração",
          href: "/geracao",
          icon: IconSparkles,
          badge: "Difusão",
          isAvailable: true,
        },
        {
          id: "playground",
          label: "Detecção YOLO",
          href: "/playground",
          icon: IconTarget,
          isAvailable: true,
        },
        {
          id: "models",
          label: "Modelos & Pesos",
          href: "/models",
          icon: IconBox,
          isAvailable: true,
        },
      ],
    },
    {
      title: "Infraestrutura",
      items: [
        {
          id: "environments",
          label: "Orquestradores",
          href: "/environments",
          icon: IconServer,
          isAvailable: true,
        },
        {
          id: "storage",
          label: "Storage S3",
          href: "/storage",
          icon: IconHardDrive,
          badge: "Roadmap",
          isAvailable: false,
        },
      ],
    },
    {
      title: "Sistema",
      items: [
        {
          id: "events",
          label: "Registro de Logs",
          href: "/events",
          icon: IconList,
          badge: "Roadmap",
          isAvailable: false,
        },
        {
          id: "settings",
          label: "Configurações",
          href: "/settings",
          icon: IconSettings,
          badge: "Roadmap",
          isAvailable: false,
        },
      ],
    },
  ];

  async function logout() {
    setLeaving(true);
    try {
      await fetch("/api/auth/logout", {
        method: "POST",
        credentials: "same-origin",
      });
    } catch {}
    router.replace("/login");
    router.refresh();
  }

  const handleItemClick = () => {
    onClose();
  };

  // Deriva status do orquestrador para o ping visual
  // Multi-nó: se ≥1 online → online; senão se ≥1 degraded → degraded; senão offline/none
  const orchStatus = (() => {
    if (orchestrators.length === 0) return "none";
    if (orchestrators.some((o) => o.status === "online")) return "online";
    if (orchestrators.some((o) => o.status === "degraded")) return "degraded";
    return "offline";
  })();

  const isItemActive = (item: NavItem) => {
    if (item.id === "dashboard") {
      return pathname === "/dashboard" || pathname === "/";
    }
    return pathname?.startsWith(item.href) ?? false;
  };

  return (
    <>
      {/* Mobile Backdrop */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: backdrop suplementar com role="presentation" — há botão fechar explícito e o backdrop usa pointer-events-none quando fechado, ficando fora da tab-order de propósito. */}
      <div
        role="presentation"
        aria-label="Fechar menu lateral"
        onClick={onClose}
        className={`fixed inset-0 z-40 bg-black/70 backdrop-blur-sm transition-opacity duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] lg:hidden ${
          open ? "opacity-100 pointer-events-auto" : "opacity-0 pointer-events-none"
        }`}
      />

      <aside
        aria-label="Navegação do studio"
        onMouseEnter={() => {
          if (typeof window !== "undefined" && window.innerWidth >= 1024) {
            setIsHovered(true);
          }
        }}
        onMouseLeave={() => {
          if (typeof window !== "undefined" && window.innerWidth >= 1024) {
            setIsHovered(false);
          }
        }}
        className={`fixed inset-y-0 left-0 z-50 flex flex-col border-r border-zinc-800/80 bg-zinc-950/85 backdrop-blur-xl transition-[width,transform,box-shadow] duration-250 ease-[cubic-bezier(0.16,1,0.3,1)] motion-reduce:transition-none will-change-[width,transform] lg:z-30 ${
          open
            ? "translate-x-0 shadow-2xl shadow-black/80"
            : "-translate-x-full lg:translate-x-0"
        } ${
          isExpanded
            ? "w-[280px] max-w-[85vw] sm:w-[300px] lg:w-[260px] lg:shadow-[16px_0_40px_rgba(0,0,0,0.75)]"
            : "w-[280px] max-w-[85vw] sm:w-[300px] lg:w-[68px] lg:shadow-none"
        }`}
      >
        {/* Header Superior (Brand Logo + Pin) */}
        <div
          className={`flex h-14 shrink-0 items-center border-b border-zinc-800/80 bg-zinc-950/60 transition-[padding] duration-250 ease-[cubic-bezier(0.16,1,0.3,1)] motion-reduce:transition-none ${
            isExpanded ? "px-3.5 justify-between" : "px-0 justify-center"
          }`}
        >
          <div className="flex items-center space-x-3 overflow-hidden">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm text-brand-400 shadow-sm shadow-brand-500/10">
              <IconTarget className="size-5" />
            </div>
            {isExpanded && (
              <div className="min-w-0 transition-opacity duration-200 opacity-100">
                <div className="flex items-center space-x-1.5 whitespace-nowrap">
                  <span className="font-display text-sm font-semibold tracking-tight text-white">
                    Hephaestus
                  </span>
                  <span className="rounded border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm px-1.5 py-0.5 font-mono text-2xs uppercase tracking-caps text-brand-300">
                    Studio
                  </span>
                </div>
                <div className="font-mono text-2xs text-zinc-400 whitespace-nowrap">
                  <span title={productVersion ? `v${productVersion}` : "Carregando versão"}>
                    {productVersion ? `v${productVersion}` : "…"}
                  </span>
                </div>
              </div>
            )}
          </div>

          {/* Botão de Fixar / Fechar */}
          {isExpanded && (
            <div className="ml-auto flex items-center space-x-1">
              {onTogglePin && (
                <button
                  type="button"
                  onClick={onTogglePin}
                  title={pinned ? "Desafixar barra lateral" : "Fixar barra lateral"}
                  aria-label={pinned ? "Desafixar barra lateral" : "Fixar barra lateral"}
                  className={`hidden size-8 items-center justify-center rounded-lg border transition hover:bg-white/[0.06] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] lg:inline-flex cursor-pointer ${
                    pinned
                      ? "border-brand-500/40 bg-brand-500/20 text-brand-300"
                      : "border-transparent text-zinc-400 hover:text-zinc-200"
                  }`}
                >
                  <IconPin className={`size-3.5 ${pinned ? "rotate-45" : ""}`} />
                </button>
              )}
              <button
                type="button"
                onClick={onClose}
                aria-label="Fechar menu lateral"
                className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-400 transition hover:bg-white/[0.08] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] lg:hidden cursor-pointer"
              >
                <IconX className="size-4.5" />
              </button>
            </div>
          )}
        </div>

        {/* Scrollable Navigation Body */}
        <div
          className={`flex-1 min-h-0 space-y-3.5 overflow-y-auto overflow-x-hidden transition-[padding] duration-250 ease-[cubic-bezier(0.16,1,0.3,1)] motion-reduce:transition-none ${
            isExpanded ? "p-3" : "py-3 px-0 flex flex-col items-center"
          }`}
        >
          {/* Card Seletor de Orquestrador / Ambiente Ativo */}
          <div className={isExpanded ? "w-full" : "flex justify-center w-full"}>
            <button
              type="button"
              onClick={() => {
                onClose();
                router.push("/dashboard");
              }}
              title={orchestrators.length === 1 ? `${orchestrators[0].name} (${orchestrators[0].endpoint})` : orchestrators.length > 1 ? `${orchestrators.length} orquestradores registrados` : "Orquestrador"}
              aria-label={orchestrators.length === 1 ? `${orchestrators[0].name} (${orchestrators[0].endpoint})` : orchestrators.length > 1 ? `${orchestrators.length} orquestradores registrados` : "Orquestrador"}
              className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] transition-colors hover:border-brand-500/40 hover:bg-brand-500/[0.06] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] cursor-pointer ${
                isExpanded
                  ? "w-full justify-between p-2.5"
                  : "size-10 justify-center p-0"
              }`}
            >
              {isExpanded ? (
                <>
                  <div className="flex items-center space-x-2.5 overflow-hidden">
                    <span className="flex size-7 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 group-hover:bg-brand-500/25 transition-colors">
                      <IconServer className="size-3.5" />
                    </span>
                    <div className="min-w-0 text-left">
                      <div className="truncate text-xs font-semibold text-zinc-200 group-hover:text-white">
                        {orchestrators.length === 1 ? orchestrators[0].name : orchestrators.length > 1 ? `${orchestrators.length} orquestradores` : "Orquestrador"}
                      </div>
                      <div className="truncate font-mono text-2xs text-zinc-400">
                        {orchestrators.length === 1 ? orchestrators[0].endpoint : "—"}
                      </div>
                    </div>
                  </div>
                  <IconChevronRight className="size-3.5 shrink-0 text-zinc-400 group-hover:text-zinc-200 transition-colors" />
                </>
              ) : (
                <IconServer className="size-4.5 text-brand-400 group-hover:scale-105 transition-transform" />
              )}
            </button>
          </div>

          {/* Botão Centro de Atividades */}
          {onOpenActionCenter && (
            <div className={isExpanded ? "w-full" : "flex justify-center w-full"}>
              <button
                type="button"
                onClick={() => {
                  onClose();
                  onOpenActionCenter();
                }}
                title="Centro de Atividades (Notificações & Jobs)"
                aria-label={`Centro de Atividades (${telemetry?.jobsActive ?? 0} jobs ativos)`}
                className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] text-xs text-zinc-300 transition-colors hover:border-brand-500/40 hover:bg-brand-500/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] cursor-pointer ${
                  isExpanded
                    ? "w-full justify-between p-2.5"
                    : "size-10 justify-center p-0"
                }`}
              >
                {isExpanded ? (
                  <>
                    <div className="flex items-center space-x-2.5 overflow-hidden">
                      <span className="flex size-7 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 group-hover:bg-brand-500/25 transition-colors">
                        <IconActivity className="size-3.5" />
                      </span>
                      <span className="font-medium whitespace-nowrap">
                        Centro de Atividades
                      </span>
                    </div>
                    <span className="rounded-full border border-brand-500/30 bg-brand-500/20 px-1.5 py-0.5 font-mono text-2xs text-brand-300 shrink-0">
                      {telemetry?.jobsActive ?? 0}
                    </span>
                  </>
                ) : (
                  <>
                    <IconActivity className="size-4.5 text-brand-400 group-hover:scale-110 transition-transform" />
                    <span className="absolute -top-1 -right-1 flex size-4 items-center justify-center rounded-full bg-brand-500 font-mono text-2xs font-bold text-white shadow-sm">
                      {telemetry?.jobsActive ?? 0}
                    </span>
                  </>
                )}
              </button>
            </div>
          )}

          {/* Seções de Navegação do Hephaestus LLM Studio */}
          {sections.map((section, sectionIdx) => (
            <div
              key={section.title}
              className={isExpanded ? "w-full" : "flex flex-col items-center w-full"}
            >
              {!isExpanded && sectionIdx > 0 && (
                <div className="w-6 h-px bg-white/5 my-1.5 shrink-0" aria-hidden="true" />
              )}
              {isExpanded && (
                <div className="mb-1 px-3 font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400 whitespace-nowrap">
                  {section.title}
                </div>
              )}
              <div
                className={`space-y-1 ${
                  isExpanded ? "w-full" : "flex flex-col items-center w-full gap-1"
                }`}
              >
                {section.items.map((item) => {
                  const active = isItemActive(item);
                  const Icon = item.icon;

                  if (!item.isAvailable) {
                    return (
                      <div
                        key={item.id}
                        role="status"
                        aria-disabled="true"
                        title={!isExpanded ? `${item.label} (Roadmap)` : undefined}
                        aria-label={!isExpanded ? `${item.label} (Roadmap)` : undefined}
                        className={`group relative flex items-center overflow-hidden rounded-xl cursor-default select-none border border-transparent opacity-45 transition-opacity ${
                          isExpanded
                            ? "w-full p-2 space-x-3 items-center text-zinc-400"
                            : "size-10 justify-center p-0 text-zinc-400"
                        }`}
                      >
                        {isExpanded ? (
                          <>
                            <span className="shrink-0 text-zinc-400">
                              <Icon className="size-4.5" />
                            </span>
                            <span className="min-w-0 flex-1 truncate text-xs font-medium text-zinc-300">
                              {item.label}
                            </span>
                            <span className="rounded border border-zinc-800 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-3xs uppercase tracking-wider text-zinc-400 shrink-0">
                              Roadmap
                            </span>
                          </>
                        ) : (
                          <div className="relative flex items-center justify-center">
                            <Icon className="size-4.5 text-zinc-400" />
                          </div>
                        )}
                      </div>
                    );
                  }

                  return (
                    <Link
                      key={item.id}
                      href={item.href}
                      onClick={handleItemClick}
                      title={!isExpanded ? item.label : undefined}
                      aria-label={!isExpanded ? item.label : undefined}
                      aria-current={active ? "page" : undefined}
                      className={`group relative flex items-center overflow-hidden rounded-xl transition-all focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] ${
                        active
                          ? isExpanded
                            ? "w-full p-2 space-x-3 items-center border border-brand-500/30 bg-brand-500/15 text-white shadow-sm"
                            : "size-10 justify-center p-0 border border-brand-500/40 bg-brand-500/20 text-brand-400 shadow-sm shadow-brand-500/10"
                          : isExpanded
                            ? "w-full p-2 space-x-3 items-center border border-transparent text-zinc-400 hover:bg-white/[0.06] hover:text-zinc-100"
                            : "size-10 justify-center p-0 border border-transparent text-zinc-400 hover:text-white hover:bg-white/[0.06]"
                      }`}
                    >
                      {active && (
                        <span
                          className={`absolute top-1/2 left-0 -translate-y-1/2 rounded-r-full bg-brand-500 ${
                            isExpanded ? "h-5 w-1" : "h-5 w-1"
                          }`}
                        />
                      )}

                      {isExpanded ? (
                        <>
                          <span
                            className={`shrink-0 transition-colors ${
                              active
                                ? "text-brand-400"
                                : "text-zinc-400 group-hover:text-zinc-200"
                            }`}
                          >
                            <Icon className="size-4.5" />
                          </span>
                          <span className="min-w-0 flex-1 truncate text-xs font-medium">
                            {item.label}
                          </span>
                          {item.badge && (
                            <span className="rounded-full border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.5 font-mono text-2xs text-brand-300 shrink-0">
                              {item.badge}
                            </span>
                          )}
                          {item.hasSubmenu && (
                            <IconChevronRight className="size-3.5 text-zinc-600 group-hover:text-zinc-400 shrink-0" />
                          )}
                        </>
                      ) : (
                        <div className="relative flex items-center justify-center">
                          <Icon className="size-4.5" />
                          {item.badge && (
                            <span
                              className="absolute -top-1 -right-1 flex size-2 rounded-full bg-brand-400 ring-2 ring-zinc-950"
                              aria-hidden="true"
                            />
                          )}
                        </div>
                      )}
                    </Link>
                  );
                })}
              </div>
            </div>
          ))}
        </div>

        {/* Rodapé: Status do Orquestrador + Perfil do Usuário + Versão */}
        <div
          className={`flex shrink-0 flex-col border-t border-zinc-800/80 bg-zinc-950/90 pb-[max(0.75rem,env(safe-area-inset-bottom))] transition-[padding] duration-250 ease-[cubic-bezier(0.16,1,0.3,1)] motion-reduce:transition-none ${
            isExpanded ? "p-3 space-y-2.5" : "py-3 px-0 items-center space-y-2"
          }`}
        >
          {/* Card Orquestrador Online */}
          {isExpanded ? (
            <div className={`flex items-center justify-between rounded-xl border backdrop-blur-sm px-2.5 py-2 text-xs ${
              orchStatus === "online"
                ? "border-status-success/25 bg-status-success/[0.05]"
                : orchStatus === "offline"
                  ? "border-status-alert/25 bg-status-alert/[0.05]"
                  : "border-white/10 bg-white/[0.03]"
            }`}>
              <div className="flex items-center space-x-2">
                {orchStatus === "online" ? (
                  <span className="relative flex h-2 w-2">
                    <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-status-success opacity-75 motion-reduce:animate-none" />
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-status-success" />
                  </span>
                ) : orchStatus === "offline" ? (
                  <span className="flex h-2 w-2">
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-status-alert" />
                  </span>
                ) : (
                  <span className="flex h-2 w-2">
                    <span className="relative inline-flex h-2 w-2 rounded-full bg-zinc-600" />
                  </span>
                )}
                <span className="text-zinc-200 font-medium">
                  {orchestrators.length === 1
                    ? `${orchestrators[0].name} · ${orchestrators[0].status}`
                    : orchestrators.length > 1
                      ? `${orchestrators.length} orquestradores`
                      : "Orquestrador"}
                </span>
              </div>
              {productVersion && (
                <span className="rounded-md border border-status-success/30 bg-status-success/10 backdrop-blur-sm px-1.5 py-0.5 font-mono text-2xs text-status-success">
                  v{productVersion}
                </span>
              )}
            </div>
          ) : (
            <div
              title={orchestrators.length === 1 ? `${orchestrators[0].name} (${orchestrators[0].status})` : orchestrators.length > 1 ? `${orchestrators.length} orquestradores` : "Orquestrador"}
              className={`relative flex size-10 items-center justify-center rounded-xl backdrop-blur-sm ${
                orchStatus === "online"
                  ? "border border-status-success/25 bg-status-success/[0.05] text-status-success"
                  : orchStatus === "offline"
                    ? "border border-status-alert/25 bg-status-alert/[0.05] text-status-alert"
                    : "border border-white/10 bg-white/[0.03] text-zinc-500"
              }`}
            >
              <IconServer className="size-4" />
              {orchStatus === "online" && (
                <span className="absolute top-1 right-1 flex h-2 w-2">
                  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-status-success opacity-75 motion-reduce:animate-none" />
                  <span className="relative inline-flex h-2 w-2 rounded-full bg-status-success" />
                </span>
              )}
            </div>
          )}

          {/* Usuário, Dados da Sessão e Menu de Logout Blindado */}
          <div
            className={`relative w-full ${isExpanded ? "" : "flex justify-center"}`}
            ref={userMenuRef}
          >
            {/* Menu Suspenso de Vidro Nível 2 (.glass-menu) */}
            {userMenuOpen && (
              <div
                role="menu"
                aria-label="Perfil do operador e opções de sessão"
                className={`glass-menu absolute z-50 rounded-2xl p-3 shadow-2xl transition-[opacity,transform] duration-200 ease-[cubic-bezier(0.16,1,0.3,1)] motion-reduce:transition-none ${
                  isExpanded
                    ? "bottom-full mb-2 left-0 right-0"
                    : "bottom-0 left-[calc(100%+8px)] w-72"
                }`}
              >
                {/* Cabeçalho do Operador */}
                <div className="flex items-start space-x-2.5 pb-2.5 border-b border-white/8">
                  <div className="flex size-8 shrink-0 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/40">
                    O
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-1">
                      <span className="truncate text-xs font-semibold text-zinc-100">
                        Operador local
                      </span>
                    </div>
                  </div>
                </div>

                {/* Metadados da Sessão e Ambiente */}
                <div className="py-2.5 space-y-1.5 text-xs">
                  <div className="rounded-xl border border-white/5 bg-white/[0.02] p-2 space-y-1">
                    <div className="flex items-center justify-between text-2xs">
                      <span className="text-zinc-400">Nó</span>
                      <span className="font-mono text-zinc-200">
                        {orchestrators.length === 1 ? orchestrators[0].name : orchestrators.length > 1 ? `${orchestrators.length} nós` : "—"}
                      </span>
                    </div>
                    <div className="flex items-center justify-between text-2xs">
                      <span className="text-zinc-400">Endpoint</span>
                      <span className="font-mono text-zinc-400">
                        {orchestrators.length === 1 ? orchestrators[0].endpoint : "—"}
                      </span>
                    </div>
                    {productVersion && (
                      <div className="flex items-center justify-between text-2xs">
                        <span className="text-zinc-400">Versão</span>
                        <span className="font-mono text-zinc-400">v{productVersion}</span>
                      </div>
                    )}
                  </div>
                </div>

                {/* Ação de Logout com Confirmação Blindada */}
                <div className="pt-1 border-t border-white/8">
                  {!confirmingLogout ? (
                    <button
                      type="button"
                      onClick={() => setConfirmingLogout(true)}
                      disabled={leaving}
                      className="group flex w-full items-center justify-between rounded-xl border border-transparent px-2.5 py-2 text-sm font-medium text-zinc-200 transition hover:border-rose-500/30 hover:bg-rose-500/10 hover:text-rose-300 active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                    >
                      <div className="flex items-center space-x-2">
                        <IconLogOut className="size-3.5 text-zinc-400 group-hover:text-rose-400 transition-colors" />
                        <span>Encerrar Sessão</span>
                      </div>
                      <IconChevronRight className="size-3.5 text-zinc-400 group-hover:text-rose-400/70 transition-colors" />
                    </button>
                  ) : (
                    <div className="rounded-xl border border-rose-500/30 bg-rose-950/20 p-2.5 space-y-2">
                      <div className="flex items-center gap-1.5 text-sm font-semibold text-rose-200">
                        <IconShield className="size-3.5 text-rose-400 shrink-0" />
                        <span>Encerrar sessão?</span>
                      </div>
                      <p className="text-xs leading-relaxed text-zinc-300">
                        Treinos e processos no daemon continuarão em segundo plano.
                      </p>
                      <div className="flex items-center gap-2 pt-1">
                        <button
                          type="button"
                          onClick={() => setConfirmingLogout(false)}
                          disabled={leaving}
                          className="flex-1 h-8 rounded-md border border-white/10 bg-white/[0.05] text-sm font-medium text-zinc-200 hover:bg-white/[0.10] hover:text-white transition active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                        >
                          Cancelar
                        </button>
                        <button
                          type="button"
                          onClick={logout}
                          disabled={leaving}
                          className="flex-1 h-8 rounded-md border border-rose-500/40 bg-rose-500/20 text-sm font-semibold text-rose-200 hover:bg-rose-500/30 transition shadow-sm active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                        >
                          {leaving ? "Saindo…" : "Confirmar"}
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Gatilho de Perfil (Expandido vs Rail) - Desacoplado sem elementos interativos aninhados */}
            {isExpanded ? (
              <div
                className={`group flex items-center rounded-xl border transition-colors ${
                  userMenuOpen
                    ? "border-brand-500/40 bg-brand-500/10 shadow-sm"
                    : "border-white/5 bg-white/[0.02] hover:border-brand-500/30 hover:bg-white/[0.05]"
                } p-1.5 justify-between w-full`}
              >
                <button
                  type="button"
                  onClick={() => setUserMenuOpen((prev) => !prev)}
                  aria-haspopup="menu"
                  aria-expanded={userMenuOpen}
                  aria-label="Perfil do operador e opções de sessão"
                  className="flex items-center space-x-2.5 overflow-hidden min-w-0 flex-1 text-left cursor-pointer rounded-lg p-1 transition-colors hover:bg-white/[0.04] focus-visible:ring-2 focus-visible:ring-brand-500/70"
                >
                  <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/30">
                    O
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-xs font-semibold text-zinc-200 group-hover:text-white">
                      Operador local
                    </div>
                  </div>
                </button>

                <div className="flex items-center space-x-1 pl-1">
                  <button
                    type="button"
                    onClick={() => {
                      setUserMenuOpen(true);
                      setConfirmingLogout(true);
                    }}
                    disabled={leaving}
                    title="Opções de saída"
                    aria-label="Encerrar sessão"
                    className="inline-flex size-7 items-center justify-center rounded-lg border border-transparent text-zinc-400 transition hover:bg-white/[0.08] hover:text-rose-400 cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)]"
                  >
                    <IconLogOut className="size-3.5" />
                  </button>
                </div>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setUserMenuOpen((prev) => !prev)}
                aria-haspopup="menu"
                aria-expanded={userMenuOpen}
                aria-label="Perfil do operador e opções de sessão"
                title="Operador local (Opções da sessão)"
                className={`flex size-10 items-center justify-center rounded-xl border transition-colors cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
                  userMenuOpen
                    ? "border-brand-500/40 bg-brand-500/10 shadow-sm"
                    : "border-transparent hover:bg-white/[0.06]"
                }`}
              >
                <div className="flex size-7 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/30 group-hover:ring-brand-400/60 transition">
                  O
                </div>
              </button>
            )}
          </div>

          {/* Versão centralizada no rodapé */}
          {isExpanded && productVersion && (
            <div className="text-center font-mono text-2xs text-zinc-400">
              Hephaestus Studio v{productVersion}
            </div>
          )}
        </div>
      </aside>
    </>
  );
}
