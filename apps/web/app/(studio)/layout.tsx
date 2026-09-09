"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import Sidebar from "@/components/studio/Sidebar";
import ActionCenter from "@/components/studio/ActionCenter";
import { ToastHost } from "@/components/studio/Toast";
import { IconMenu, IconZap } from "@/components/icons";
import { Badge, Button } from "@/components/ui";

import { ACTION_CENTER_EVENT } from "@/lib/events";

const SEGMENT_LABELS: Record<string, string> = {
  dashboard: "Painel",
  datasets: "Datasets",
  annotate: "Anotar",
  login: "Login",
  jobs: "Execuções",
  treino: "Treino YOLO",
};

function labelFor(segment: string): string {
  return SEGMENT_LABELS[segment] ?? segment;
}

export default function StudioLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const pathname = usePathname();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [actionCenterOpen, setActionCenterOpen] = useState(false);
  const [sidebarPinned, setSidebarPinned] = useState(false);

  useEffect(() => {
    const handleOpen = () => setActionCenterOpen(true);
    window.addEventListener(ACTION_CENTER_EVENT, handleOpen);
    return () => window.removeEventListener(ACTION_CENTER_EVENT, handleOpen);
  }, []);

  useEffect(() => {
    try {
      const saved = localStorage.getItem("hephaestus_sidebar_pinned");
      if (saved === "true") setSidebarPinned(true);
    } catch {}
  }, []);

  const handleTogglePin = () => {
    setSidebarPinned((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("hephaestus_sidebar_pinned", String(next));
      } catch {}
      return next;
    });
  };

  useEffect(() => {
    setSidebarOpen(false);
  }, [pathname]);

  useEffect(() => {
    const handleResize = () => {
      if (window.innerWidth >= 1024 && sidebarOpen) {
        setSidebarOpen(false);
      }
    };
    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, [sidebarOpen]);

  useEffect(() => {
    if (!sidebarOpen) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setSidebarOpen(false);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [sidebarOpen]);

  const segments = pathname.split("/").filter(Boolean);
  const lastIndex = segments.length - 1;

  return (
    <div className="relative flex h-screen h-[100dvh] overflow-hidden bg-[#0a0812] text-zinc-100">
      {/* Background óptico contínuo: Iluminação Violeta Arcane (rgba(131,80,242,0.22)) + Grade Técnica SVG */}
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 z-0 overflow-hidden"
      >
        {/* 1. Iluminação Violeta Arcane */}
        <div
          className="absolute inset-0"
          style={{
            background:
              "radial-gradient(ellipse 65% 50% at 18% 18%, rgba(131,80,242,0.22), transparent 70%), " +
              "radial-gradient(ellipse 60% 45% at 82% 22%, rgba(131,80,242,0.22), transparent 65%), " +
              "radial-gradient(ellipse 70% 55% at 50% 80%, rgba(131,80,242,0.20), transparent 70%), " +
              "radial-gradient(ellipse 50% 50% at 15% 85%, rgba(131,80,242,0.18), transparent 65%), " +
              "radial-gradient(ellipse 40% 40% at 85% 85%, rgba(131,80,242,0.16), transparent 60%)",
          }}
        />

        {/* 2. Grade técnica SVG perfeitamente contínua sob toda a tela */}
        <div
          className="absolute inset-0"
          style={{
            opacity: 0.75,
            backgroundImage:
              'url("data:image/svg+xml,%3Csvg xmlns=\'http://www.w3.org/2000/svg\' width=\'48\' height=\'48\'%3E%3Cpath d=\'M47.5 0v48M0 47.5h48\' stroke=\'rgba(160,150,185,1)\' stroke-width=\'1\' stroke-opacity=\'0.18\' fill=\'none\'/%3E%3C/svg%3E")',
            backgroundSize: "48px 48px",
          }}
        />

        {/* 3. Vignette suave para profundidade de borda */}
        <div
          className="absolute inset-0"
          style={{
            background:
              "radial-gradient(ellipse at center, transparent 55%, rgba(10,8,18,0.5) 100%)",
          }}
        />
      </div>

      {/* Espaçador estático no desktop para manter o layout livre de layout-shift durante expansão */}
      <div
        className={`hidden shrink-0 transition-[width] duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] lg:block ${
          sidebarPinned ? "w-[260px]" : "w-[68px]"
        }`}
        aria-hidden="true"
      />

      <Sidebar
        open={sidebarOpen}
        onClose={() => setSidebarOpen(false)}
        onOpenActionCenter={() => setActionCenterOpen(true)}
        pinned={sidebarPinned}
        onTogglePin={handleTogglePin}
      />

      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-zinc-800/80 bg-zinc-950/80 px-3 backdrop-blur-xl sm:px-4">
          <div className="flex min-w-0 items-center space-x-2.5 sm:space-x-3">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => setSidebarOpen(true)}
              aria-label="Abrir menu lateral"
              className="lg:hidden"
            >
              <IconMenu />
            </Button>

            <nav
              aria-label="Navegação atual"
              className="flex min-w-0 items-center space-x-2 font-mono text-xs"
            >
              <span className="hidden shrink-0 text-zinc-500 sm:inline">
                Hephaestus Studio
              </span>
              <span aria-hidden="true" className="hidden shrink-0 text-zinc-600 sm:inline">
                /
              </span>
              {segments.length === 0 ? (
                <span className="font-display font-semibold tracking-tight text-zinc-200">
                  Studio
                </span>
              ) : (
                segments.map((segment, i) => {
                  const label = labelFor(segment);
                  const isLast = i === lastIndex;
                  if (isLast) {
                    return (
                      <span
                        key={`${segment}-${i}`}
                        title={label}
                        className="font-display truncate font-semibold tracking-tight text-zinc-200"
                      >
                        {label}
                      </span>
                    );
                  }
                  return (
                    <span key={`${segment}-${i}`} className="flex min-w-0 items-center space-x-2">
                      <span
                        title={label}
                        className="block max-w-[140px] truncate text-zinc-500"
                      >
                        {label}
                      </span>
                      <span aria-hidden="true" className="shrink-0 text-zinc-600">
                        /
                      </span>
                    </span>
                  );
                })
              )}
            </nav>
          </div>

          <div className="flex shrink-0 items-center space-x-2">
            <Button
              type="button"
              variant="secondary"
              size="icon"
              onClick={() => setActionCenterOpen(true)}
              title="Centro de Atividades"
              aria-label="Abrir Centro de Atividades"
            >
              <IconZap className="size-4 text-brand-400" />
            </Button>
            <Badge
              variant="telemetry"
              dot
              title="Nó local — ambiente único nesta fatia"
            >
              Local
            </Badge>
          </div>
        </header>

        <main className="min-h-0 flex-1 overflow-y-auto">{children}</main>
        <ToastHost />
        <ActionCenter
          open={actionCenterOpen}
          onClose={() => setActionCenterOpen(false)}
        />
      </div>
    </div>
  );
}
