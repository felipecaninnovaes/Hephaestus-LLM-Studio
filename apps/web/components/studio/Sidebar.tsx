"use client";

import { useEffect, useRef, useState } from "react";
import { usePathname, useRouter } from "next/navigation";
import Link from "next/link";
import {
  IconActivity,
  IconBox,
  IconBoxSelect,
  IconChevronRight,
  IconChevronsUpDown,
  IconCpu,
  IconDatabase,
  IconFolder,
  IconHardDrive,
  IconHome,
  IconImage,
  IconLayers,
  IconList,
  IconLogOut,
  IconNetwork,
  IconPin,
  IconPlay,
  IconRefresh,
  IconServer,
  IconSettings,
  IconShield,
  IconSparkles,
  IconTarget,
  IconX,
  IconZap,
} from "@/components/icons";
import { showToast } from "@/components/studio/Toast";
import { getTelemetry } from "@/lib/jobs";
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
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [confirmingLogout, setConfirmingLogout] = useState(false);
  const userMenuRef = useRef<HTMLDivElement>(null);

  // Fecha o menu de perfil ao trocar de rota ou fechar o drawer
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
      try {
        const data = await getTelemetry();
        if (active) setTelemetry(data);
      } catch {}
    };
    fetchTelem();
    const interval = setInterval(fetchTelem, 3000);
    return () => {
      active = false;
      clearInterval(interval);
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
        {
          id: "autotracker",
          label: "AutoTracker",
          href: "/autotracker",
          icon: IconBoxSelect,
          badge: "IA",
          isAvailable: false,
        },
        {
          id: "autolabel",
          label: "AutoLabel",
          href: "/autolabel",
          icon: IconSparkles,
          badge: "IA",
          isAvailable: false,
        },
      ],
    },
    {
      title: "Forja & Treino",
      items: [
        {
          id: "jobs",
          label: "Treino YOLO",
          href: "/jobs",
          icon: IconTarget,
          badge: telemetry?.jobsActive ? `${telemetry.jobsActive}` : undefined,
          isAvailable: true,
        },
        {
          id: "difusao",
          label: "Difusão LoRA",
          href: "/difusao",
          icon: IconImage,
          isAvailable: false,
        },
        {
          id: "openclip",
          label: "OpenCLIP",
          href: "/openclip",
          icon: IconNetwork,
          isAvailable: false,
        },
        {
          id: "playground",
          label: "Playground",
          href: "/playground",
          icon: IconPlay,
          badge: "Idle",
          isAvailable: false,
        },
        {
          id: "models",
          label: "Modelos & Pesos",
          href: "/models",
          icon: IconBox,
          hasSubmenu: true,
          isAvailable: false,
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
          hasSubmenu: true,
          isAvailable: false,
        },
        {
          id: "storage",
          label: "Storage S3",
          href: "/storage",
          icon: IconHardDrive,
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
          isAvailable: false,
        },
        {
          id: "settings",
          label: "Configurações",
          href: "/settings",
          icon: IconSettings,
          hasSubmenu: true,
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

  const handleItemClick = (e: React.MouseEvent, item: NavItem) => {
    if (item.id === "autotracker") {
      e.preventDefault();
      onClose();
      router.push("/datasets");
      showToast(
        "O AutoTracker opera na galeria. Selecione um dataset YOLO para executá-lo.",
        "info",
      );
      return;
    }
    if (item.id === "autolabel") {
      e.preventDefault();
      onClose();
      router.push("/datasets");
      showToast(
        "O AutoLabel opera na galeria. Selecione um dataset para executá-lo.",
        "info",
      );
      return;
    }
    if (!item.isAvailable) {
      e.preventDefault();
      showToast(
        `O módulo "${item.label}" estará disponível na próxima fatia de integração.`,
        "info",
      );
      return;
    }
    onClose();
  };

  const isItemActive = (item: NavItem) => {
    if (item.id === "dashboard") {
      return pathname === "/dashboard" || pathname === "/";
    }
    return pathname?.startsWith(item.href) ?? false;
  };

  return (
    <>
      {/* Mobile Backdrop */}
      <div
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
        className={`fixed inset-y-0 left-0 z-50 flex flex-col border-r border-zinc-800/80 bg-zinc-950/95 backdrop-blur-xl transition-[width,transform,box-shadow] duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] lg:z-30 ${
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
          className={`flex h-14 shrink-0 items-center border-b border-zinc-800/80 bg-zinc-950/60 transition-all duration-300 ${
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
                  <span className="rounded border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm px-1.5 py-0.5 font-mono text-[11px] uppercase tracking-caps text-brand-300">
                    Studio
                  </span>
                </div>
                <div className="font-mono text-[11px] text-zinc-500 whitespace-nowrap">
                  <span title="Hephaestus LLM Studio v1.3.0">v1.3.0 · AI Engine</span>
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
          className={`flex-1 min-h-0 space-y-3.5 overflow-y-auto overflow-x-hidden transition-all duration-300 ${
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
              title="Orquestrador Local (http://localhost:8080 · CUDA 12.4)"
              aria-label="Orquestrador Local (http://localhost:8080 · CUDA 12.4)"
              className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] transition-all hover:border-brand-500/40 hover:bg-brand-500/[0.06] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] cursor-pointer ${
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
                        Orquestrador Local
                      </div>
                      <div className="truncate font-mono text-[11px] text-zinc-500">
                        localhost:8080 · PyTorch CUDA
                      </div>
                    </div>
                  </div>
                  <IconChevronsUpDown className="size-3.5 shrink-0 text-zinc-500 group-hover:text-zinc-300" />
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
                className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] text-xs text-zinc-300 transition-all hover:border-brand-500/40 hover:bg-brand-500/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] cursor-pointer ${
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
                    <span className="rounded-full border border-brand-500/30 bg-brand-500/20 px-1.5 py-0.5 font-mono text-[11px] text-brand-300 shrink-0">
                      {telemetry?.jobsActive ?? 0}
                    </span>
                  </>
                ) : (
                  <>
                    <IconActivity className="size-4.5 text-brand-400 group-hover:scale-110 transition-transform" />
                    <span className="absolute -top-1 -right-1 flex size-4 items-center justify-center rounded-full bg-brand-500 font-mono text-[11px] font-bold text-white shadow-sm">
                      {telemetry?.jobsActive ?? 0}
                    </span>
                  </>
                )}
              </button>
            </div>
          )}

          {/* Seções de Navegação do Hephaestus LLM Studio */}
          {sections.map((section) => (
            <div
              key={section.title}
              className={isExpanded ? "w-full" : "flex flex-col items-center w-full"}
            >
              {isExpanded && (
                <div className="mb-1 px-3 font-mono text-[11px] font-semibold uppercase tracking-wider text-zinc-500 whitespace-nowrap">
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

                  return (
                    <Link
                      key={item.id}
                      href={item.href}
                      onClick={(e) => handleItemClick(e, item)}
                      title={`${item.label}${!item.isAvailable ? " (Em breve)" : ""}`}
                      aria-label={!isExpanded ? `${item.label}${!item.isAvailable ? " (Em breve)" : ""}` : undefined}
                      aria-current={active ? "page" : undefined}
                      className={`group relative flex items-center overflow-hidden rounded-xl transition-all focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] ${
                        active
                          ? isExpanded
                            ? "w-full p-2 space-x-3 items-center border border-brand-500/30 bg-brand-500/15 text-white shadow-sm"
                            : "size-10 justify-center p-0 border border-brand-500/40 bg-brand-500/20 text-brand-400 shadow-sm shadow-brand-500/10"
                          : isExpanded
                            ? "w-full p-2 space-x-3 items-center border border-transparent text-zinc-400 hover:bg-white/[0.06] hover:text-zinc-100"
                            : "size-10 justify-center p-0 border border-transparent text-zinc-400 hover:text-white hover:bg-white/[0.06]"
                      } ${!item.isAvailable ? "opacity-75" : ""}`}
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
                            <span className="rounded-full border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.5 font-mono text-[11px] text-brand-300 shrink-0">
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
          className={`flex shrink-0 flex-col border-t border-zinc-800/80 bg-zinc-950/90 pb-[max(0.75rem,env(safe-area-inset-bottom))] transition-all duration-300 ${
            isExpanded ? "p-3 space-y-2.5" : "py-3 px-0 items-center space-y-2"
          }`}
        >
          {/* Card Orquestrador Online */}
          {isExpanded ? (
            <div className="flex items-center justify-between rounded-xl border border-[#34d399]/25 bg-[#34d399]/[0.05] backdrop-blur-sm px-2.5 py-2 text-xs">
              <div className="flex items-center space-x-2">
                <span className="relative flex h-2 w-2">
                  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-[#34d399] opacity-75" />
                  <span className="relative inline-flex h-2 w-2 rounded-full bg-[#34d399]" />
                </span>
                <span className="text-zinc-200 font-medium">Orquestrador Online</span>
              </div>
              <span className="rounded-md border border-[#34d399]/30 bg-[#34d399]/10 backdrop-blur-sm px-1.5 py-0.5 font-mono text-[11px] text-[#34d399]">
                v1.3.0
              </span>
            </div>
          ) : (
            <div
              title="Orquestrador Online (v1.3.0)"
              className="relative flex size-10 items-center justify-center rounded-xl border border-[#34d399]/25 bg-[#34d399]/[0.05] backdrop-blur-sm text-[#34d399]"
            >
              <IconServer className="size-4" />
              <span className="absolute top-1 right-1 flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-[#34d399] opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-[#34d399]" />
              </span>
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
                className={`glass-menu absolute z-50 rounded-2xl p-3 shadow-2xl transition-all ${
                  isExpanded
                    ? "bottom-full mb-2 left-0 right-0"
                    : "bottom-0 left-[calc(100%+8px)] w-72"
                }`}
              >
                {/* Cabeçalho do Operador */}
                <div className="flex items-start space-x-2.5 pb-2.5 border-b border-white/8">
                  <div className="flex size-8 shrink-0 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/40">
                    H
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center justify-between gap-1">
                      <span className="truncate text-xs font-semibold text-zinc-100">
                        Hephaestus Admin
                      </span>
                      <span className="rounded border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm px-1.5 py-0.5 font-mono text-[11px] font-semibold uppercase tracking-caps text-brand-300">
                        Root
                      </span>
                    </div>
                    <div className="truncate font-mono text-[11px] text-zinc-400">
                      admin@localhost
                    </div>
                  </div>
                </div>

                {/* Metadados da Sessão e Ambiente */}
                <div className="py-2.5 space-y-1.5 text-xs">
                  <div className="flex items-center justify-between">
                    <span className="font-mono text-[11px] font-semibold uppercase tracking-wider text-zinc-500">
                      Ambiente Ativo
                    </span>
                    <span className="inline-flex items-center gap-1.5 font-mono text-[11px] text-[#34d399]">
                      <span className="size-1.5 rounded-full bg-[#34d399] animate-pulse" />
                      Online
                    </span>
                  </div>
                  <div className="rounded-xl border border-white/5 bg-white/[0.02] p-2 space-y-1">
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="text-zinc-400">Nó</span>
                      <span className="font-mono text-zinc-200">Orquestrador Local</span>
                    </div>
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="text-zinc-400">Endpoint</span>
                      <span className="font-mono text-zinc-400">localhost:8080</span>
                    </div>
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="text-zinc-400">Runtime</span>
                      <span className="font-mono text-zinc-300">PyTorch CUDA</span>
                    </div>
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="text-zinc-400">Versão</span>
                      <span className="font-mono text-zinc-500">v1.3.0</span>
                    </div>
                  </div>
                </div>

                {/* Ação de Logout com Confirmação Blindada */}
                <div className="pt-1 border-t border-white/8">
                  {!confirmingLogout ? (
                    <button
                      type="button"
                      onClick={() => setConfirmingLogout(true)}
                      disabled={leaving}
                      className="group flex w-full items-center justify-between rounded-xl border border-transparent px-2.5 py-2 text-xs font-medium text-zinc-300 transition hover:border-rose-500/30 hover:bg-rose-500/10 hover:text-rose-300 active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                    >
                      <div className="flex items-center space-x-2">
                        <IconLogOut className="size-3.5 text-zinc-400 group-hover:text-rose-400 transition-colors" />
                        <span>Encerrar Sessão</span>
                      </div>
                      <IconChevronRight className="size-3.5 text-zinc-600 group-hover:text-rose-400/70 transition-colors" />
                    </button>
                  ) : (
                    <div className="rounded-xl border border-rose-500/30 bg-rose-950/20 p-2.5 space-y-2">
                      <div className="flex items-center gap-1.5 text-xs font-semibold text-rose-200">
                        <IconShield className="size-3.5 text-rose-400 shrink-0" />
                        <span>Encerrar sessão?</span>
                      </div>
                      <p className="text-[11px] leading-tight text-zinc-400">
                        Treinos e processos no daemon continuarão em segundo plano.
                      </p>
                      <div className="flex items-center gap-2 pt-1">
                        <button
                          type="button"
                          onClick={() => setConfirmingLogout(false)}
                          disabled={leaving}
                          className="flex-1 h-7.5 rounded-lg border border-white/10 bg-white/[0.05] text-xs font-medium text-zinc-300 hover:bg-white/[0.10] hover:text-white transition active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                        >
                          Cancelar
                        </button>
                        <button
                          type="button"
                          onClick={logout}
                          disabled={leaving}
                          className="flex-1 h-7.5 rounded-lg border border-rose-500/40 bg-rose-500/20 text-xs font-semibold text-rose-200 hover:bg-rose-500/30 transition shadow-sm active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70"
                        >
                          {leaving ? "Saindo…" : "Confirmar"}
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              </div>
            )}

            {/* Gatilho de Perfil (Expandido vs Rail) */}
            <div
              onClick={() => setUserMenuOpen((prev) => !prev)}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setUserMenuOpen((prev) => !prev);
                }
              }}
              aria-haspopup="menu"
              aria-expanded={userMenuOpen}
              aria-label="Perfil do operador e opções de sessão"
              className={`group flex items-center rounded-xl border transition-all active:scale-[0.985] cursor-pointer focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
                isExpanded
                  ? "p-2 justify-between w-full border-white/5 bg-white/[0.02] hover:border-brand-500/30 hover:bg-white/[0.05]"
                  : "p-0 size-10 justify-center border-transparent hover:bg-white/[0.06]"
              } ${
                userMenuOpen
                  ? "border-brand-500/40 bg-brand-500/10 shadow-sm"
                  : ""
              }`}
            >
              {isExpanded ? (
                <>
                  <div className="flex items-center space-x-2.5 overflow-hidden">
                    <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/30">
                      H
                    </div>
                    <div className="min-w-0 text-left">
                      <div className="truncate text-xs font-semibold text-zinc-200 group-hover:text-white">
                        Hephaestus Admin
                      </div>
                      <div className="truncate font-mono text-[11px] text-zinc-500">
                        admin@localhost
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center space-x-1">
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
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
                </>
              ) : (
                <div
                  title="Hephaestus Admin (Opções da sessão)"
                  className="flex size-7 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm ring-1 ring-brand-400/30 group-hover:ring-brand-400/60 transition"
                >
                  H
                </div>
              )}
            </div>
          </div>

          {/* Versão centralizada no rodapé */}
          {isExpanded && (
            <div className="text-center font-mono text-[11px] text-zinc-600">
              Hephaestus Studio v1.3.0
            </div>
          )}
        </div>
      </aside>
    </>
  );
}
