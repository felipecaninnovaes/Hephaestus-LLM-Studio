"use client";

import { useEffect, useState } from "react";
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

  const isExpanded = isHovered || pinned;

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
          label: "Forja & Treinamento",
          href: "/jobs",
          icon: IconLayers,
          badge: telemetry?.jobsActive ? `${telemetry.jobsActive}` : undefined,
          isAvailable: true,
        },
        {
          id: "yolo",
          label: "YOLO Vision",
          href: "/yolo",
          icon: IconTarget,
          isAvailable: false,
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
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        className={`fixed inset-y-0 left-0 z-40 flex flex-col border-r border-zinc-800/80 bg-zinc-950/95 backdrop-blur-xl transition-[width,transform,box-shadow] duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] ${
          open ? "translate-x-0" : "-translate-x-full lg:translate-x-0"
        } ${
          isExpanded
            ? "w-[min(85vw,300px)] lg:w-[260px] lg:shadow-[16px_0_40px_rgba(0,0,0,0.75)]"
            : "w-[min(85vw,300px)] lg:w-[68px] lg:shadow-none"
        }`}
      >
        {/* Header Superior (Brand Logo + Pin) */}
        <div
          className={`flex h-14 shrink-0 items-center border-b border-zinc-800/80 bg-zinc-950/60 transition-all duration-300 ${
            isExpanded ? "px-3.5 justify-between" : "px-0 justify-center"
          }`}
        >
          <div className="flex items-center space-x-3 overflow-hidden">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 text-brand-400 shadow-sm shadow-brand-500/10">
              <IconTarget className="size-5" />
            </div>
            {isExpanded && (
              <div className="min-w-0 transition-opacity duration-200 opacity-100">
                <div className="flex items-center space-x-1.5 whitespace-nowrap">
                  <span className="font-display text-sm font-semibold tracking-tight text-white">
                    Hephaestus
                  </span>
                  <span className="rounded border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-caps text-brand-300">
                    Studio
                  </span>
                </div>
                <div className="font-mono text-[10px] text-zinc-500 whitespace-nowrap">
                  <span title="Hephaestus LLM Studio v1.3.0">v1.3.0 · AI Engine</span>
                </div>
              </div>
            )}
          </div>

          {/* Botão de Fixar / Fechar */}
          {isExpanded && (
            <div className="ml-auto flex items-center">
              {onTogglePin && (
                <button
                  type="button"
                  onClick={onTogglePin}
                  title={pinned ? "Desafixar barra lateral" : "Fixar barra lateral"}
                  aria-label={pinned ? "Desafixar barra lateral" : "Fixar barra lateral"}
                  className={`hidden size-8 items-center justify-center rounded-lg border transition hover:bg-white/[0.06] active:scale-[0.985] lg:inline-flex cursor-pointer ${
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
                className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] lg:hidden"
              >
                <IconX />
              </button>
            </div>
          )}
        </div>

        {/* Scrollable Navigation Body */}
        <div
          className={`flex-1 space-y-3.5 overflow-y-auto overflow-x-hidden transition-all duration-300 ${
            isExpanded ? "p-3" : "py-3 px-0 flex flex-col items-center"
          }`}
        >
          {/* Card Seletor de Orquestrador / Ambiente Ativo */}
          <div className={isExpanded ? "w-full" : "flex justify-center w-full"}>
            <button
              type="button"
              onClick={() => router.push("/dashboard")}
              title="Orquestrador Local (http://localhost:8080 · CUDA 12.4)"
              className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] transition-all hover:border-brand-500/40 hover:bg-brand-500/[0.06] active:scale-[0.985] cursor-pointer ${
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
                      <div className="truncate font-mono text-[10px] text-zinc-500">
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
                className={`group relative flex items-center rounded-xl border border-white/10 bg-white/[0.03] text-xs text-zinc-300 transition-all hover:border-brand-500/40 hover:bg-brand-500/[0.06] hover:text-white active:scale-[0.985] cursor-pointer ${
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
                    <span className="rounded-full border border-brand-500/30 bg-brand-500/20 px-1.5 py-0.5 font-mono text-[10px] text-brand-300 shrink-0">
                      {telemetry?.jobsActive ?? 3}
                    </span>
                  </>
                ) : (
                  <>
                    <IconActivity className="size-4.5 text-brand-400 group-hover:scale-110 transition-transform" />
                    <span className="absolute -top-1 -right-1 flex size-4 items-center justify-center rounded-full bg-brand-500 font-mono text-[9px] font-bold text-white shadow-sm">
                      {telemetry?.jobsActive ?? 3}
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
                <div className="mb-1 px-3 font-mono text-[10px] font-semibold uppercase tracking-wider text-zinc-500 whitespace-nowrap">
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
                      className={`group relative flex items-center overflow-hidden rounded-xl transition-all ${
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
                            <span className="rounded-full border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.2 font-mono text-[9px] text-brand-300 shrink-0">
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
                            <span className="absolute -top-1.5 -right-2 flex size-3 items-center justify-center rounded-full bg-brand-500/80 font-mono text-[8px] font-bold text-white">
                              •
                            </span>
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
          className={`flex shrink-0 flex-col border-t border-zinc-800/80 bg-zinc-950/90 transition-all duration-300 ${
            isExpanded ? "p-3 space-y-3" : "py-3 px-0 items-center space-y-2"
          }`}
        >
          {/* Card Orquestrador Online */}
          {isExpanded ? (
            <div className="flex items-center justify-between rounded-xl border border-emerald-500/20 bg-emerald-500/5 px-2.5 py-2 text-xs">
              <div className="flex items-center space-x-2">
                <span className="relative flex h-2 w-2">
                  <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                  <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
                </span>
                <span className="text-zinc-300 font-medium">Orquestrador Online</span>
              </div>
              <span className="rounded-md border border-emerald-500/30 bg-emerald-500/10 px-1.5 py-0.5 font-mono text-[10px] text-emerald-300">
                v1.3.0
              </span>
            </div>
          ) : (
            <div
              title="Orquestrador Online (v1.3.0)"
              className="relative flex size-10 items-center justify-center rounded-xl border border-emerald-500/20 bg-emerald-500/5 text-emerald-400"
            >
              <IconServer className="size-4" />
              <span className="absolute top-1 right-1 flex h-2 w-2">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-500" />
              </span>
            </div>
          )}

          {/* Usuário e Logout */}
          <div
            className={`flex items-center rounded-xl border border-white/5 bg-white/[0.02] ${
              isExpanded ? "p-2 justify-between" : "p-0 size-10 justify-center"
            }`}
          >
            {isExpanded ? (
              <>
                <div className="flex items-center space-x-2.5 overflow-hidden">
                  <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white shadow-sm">
                    H
                  </div>
                  <div className="min-w-0">
                    <div className="truncate text-xs font-semibold text-zinc-200">
                      Hephaestus Admin
                    </div>
                    <div className="truncate font-mono text-[10px] text-zinc-500">
                      admin@localhost
                    </div>
                  </div>
                </div>

                <div className="flex items-center space-x-1">
                  <button
                    type="button"
                    onClick={logout}
                    disabled={leaving}
                    title="Sair da sessão"
                    aria-label="Sair da sessão"
                    className="inline-flex size-7 items-center justify-center rounded-lg border border-transparent text-zinc-400 transition hover:bg-white/[0.08] hover:text-rose-400 cursor-pointer"
                  >
                    <IconLogOut className="size-3.5" />
                  </button>
                </div>
              </>
            ) : (
              <button
                type="button"
                onClick={logout}
                disabled={leaving}
                title="Hephaestus Admin (Sair)"
                aria-label="Sair da sessão"
                className="flex size-10 items-center justify-center rounded-xl text-zinc-400 transition hover:bg-white/[0.06] hover:text-rose-400 cursor-pointer"
              >
                <div className="flex size-7 items-center justify-center rounded-full bg-brand-600 font-display text-xs font-bold text-white">
                  H
                </div>
              </button>
            )}
          </div>

          {/* Versão centralizada no rodapé */}
          {isExpanded && (
            <div className="text-center font-mono text-[10px] text-zinc-600">
              Hephaestus Studio v1.3.0
            </div>
          )}
        </div>
      </aside>
    </>
  );
}
