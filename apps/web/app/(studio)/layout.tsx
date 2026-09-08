"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import Sidebar from "@/components/studio/Sidebar";
import { ToastHost } from "@/components/studio/Toast";
import { IconMenu } from "@/components/icons";

const SEGMENT_LABELS: Record<string, string> = {
  datasets: "Datasets",
  annotate: "Anotar",
  login: "Login",
  jobs: "Forja & Treinamento",
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

  useEffect(() => {
    setSidebarOpen(false);
  }, [pathname]);

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
    <div className="flex h-screen overflow-hidden bg-zinc-950 text-zinc-100">
      <Sidebar open={sidebarOpen} onClose={() => setSidebarOpen(false)} />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center justify-between border-b border-zinc-800/80 bg-zinc-950/80 px-3 backdrop-blur-xl sm:px-4">
          <div className="flex min-w-0 items-center space-x-2.5 sm:space-x-3">
            <button
              type="button"
              onClick={() => setSidebarOpen(true)}
              aria-label="Abrir menu lateral"
              className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 lg:hidden"
            >
              <IconMenu />
            </button>

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

          <div className="flex shrink-0 items-center">
            <span
              title="Nó local — ambiente único nesta fatia"
              className="flex items-center space-x-2 rounded-full border border-zinc-800 bg-zinc-900/90 px-2.5 py-1 font-mono text-[11px] text-zinc-300"
            >
              <span aria-hidden="true" className="h-2 w-2 rounded-full bg-brand-400" />
              <span>Local</span>
            </span>
          </div>
        </header>

        <main className="min-h-0 flex-1 overflow-y-auto">{children}</main>
        <ToastHost />
      </div>
    </div>
  );
}
