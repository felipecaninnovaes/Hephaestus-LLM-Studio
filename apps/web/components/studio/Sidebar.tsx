"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconCpu,
  IconDatabase,
  IconLayers,
  IconLogOut,
  IconSettings,
  IconTarget,
  IconX,
} from "@/components/icons";

interface SidebarProps {
  open: boolean;
  onClose: () => void;
}

const TELEMETRY_ROWS = [
  { label: "VRAM", value: "—" },
  { label: "CPU", value: "—" },
  { label: "RAM", value: "—" },
] as const;

export default function Sidebar({ open, onClose }: SidebarProps) {
  const router = useRouter();
  const [leaving, setLeaving] = useState(false);

  async function logout() {
    setLeaving(true);
    try {
      await fetch("/api/auth/logout", {
        method: "POST",
        credentials: "same-origin",
      });
    } catch {
      // Mesmo sem resposta, a sessão local termina aqui.
    }
    router.replace("/login");
    router.refresh();
  }

  return (
    <>
      {/* Backdrop do drawer (< lg) */}
      {open && (
        <button
          type="button"
          aria-label="Fechar menu lateral"
          onClick={onClose}
          className="fixed inset-0 z-40 bg-black/70 backdrop-blur-sm lg:hidden"
        />
      )}

      <aside
        aria-label="Navegação do studio"
        className={`fixed inset-y-0 left-0 z-50 flex w-[min(85vw,320px)] shrink-0 flex-col border-r border-zinc-800/80 bg-zinc-950 transition-transform duration-200 ease-in-out lg:static lg:z-auto lg:w-[255px] lg:translate-x-0 ${
          open ? "translate-x-0" : "-translate-x-full"
        }`}
      >
        {/* Identidade */}
        <div className="flex h-14 shrink-0 items-center justify-between border-b border-zinc-800/80 bg-zinc-950/60 px-4">
          <div className="flex items-center space-x-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/10 text-brand-400 shadow-sm shadow-brand-500/10">
              <IconTarget />
            </div>
            <div>
              <div className="flex items-center space-x-1.5">
                <span className="font-display text-sm font-semibold tracking-tight text-white">
                  Hephaestus
                </span>
                <span className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-caps text-zinc-400">
                  Studio
                </span>
              </div>
              <div className="font-mono text-[10px] text-zinc-500">
                <span title="v1.3 · Local Node">v1.3 · Local Node</span>
              </div>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Fechar menu lateral"
            className="touch-target touch-manipulation flex items-center justify-center rounded-xl p-2 text-zinc-400 transition-colors hover:bg-zinc-800 hover:text-white lg:hidden"
          >
            <IconX />
          </button>
        </div>

        {/* Módulos */}
        <div className="flex-1 space-y-4 p-3">
          <div>
            <div className="mb-2 px-3 font-mono text-[10px] font-semibold uppercase tracking-caps text-zinc-400">
              Módulos de Sistema
            </div>
            <div className="space-y-1.5">
              {/* Módulo ativo */}
              <span
                aria-current="page"
                className="relative flex w-full items-start space-x-3 overflow-hidden rounded-xl border border-brand-500/30 bg-zinc-900/90 p-2.5 text-left text-white shadow-sm"
              >
                <span className="absolute top-1/2 left-0 h-6 w-1 -translate-y-1/2 rounded-r-full bg-brand-500" />
                <span className="shrink-0 rounded-lg border border-brand-500/30 bg-brand-500/15 p-2 text-brand-400">
                  <IconDatabase />
                </span>
                <span className="min-w-0 flex-1">
                  <span
                    title="Dados & Anotação"
                    className="block truncate text-xs font-semibold text-zinc-100"
                  >
                    Dados &amp; Anotação
                  </span>
                  <span
                    title="Curadoria, BBoxes e legendas"
                    className="mt-0.5 block truncate text-[11px] text-zinc-400"
                  >
                    Curadoria, BBoxes e legendas
                  </span>
                </span>
              </span>

              {/* Desabilitados honestos */}
              <span
                title="Fatia futura"
                aria-disabled="true"
                className="flex w-full cursor-not-allowed items-start space-x-3 rounded-xl border border-transparent p-2.5 text-left text-zinc-400 opacity-55"
              >
                <span className="shrink-0 rounded-lg bg-zinc-900 p-2 text-zinc-400">
                  <IconLayers />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center justify-between gap-2">
                    <span
                      title="Forja & Treinamento"
                      className="min-w-0 flex-1 truncate text-xs font-semibold text-zinc-100"
                    >
                      Forja &amp; Treinamento
                    </span>
                    <span className="shrink-0 rounded bg-zinc-800/80 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">
                      3 Motores
                    </span>
                  </span>
                  <span
                    title="Difusão, OpenCLIP e YOLO"
                    className="mt-0.5 block truncate text-[11px] text-zinc-400"
                  >
                    Difusão, OpenCLIP e YOLO
                  </span>
                </span>
              </span>

              <span
                title="Fatia futura"
                aria-disabled="true"
                className="flex w-full cursor-not-allowed items-start space-x-3 rounded-xl border border-transparent p-2.5 text-left text-zinc-400 opacity-55"
              >
                <span className="shrink-0 rounded-lg bg-zinc-900 p-2 text-zinc-400">
                  <IconCpu />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center justify-between gap-2">
                    <span
                      title="Execução & Playground"
                      className="min-w-0 flex-1 truncate text-xs font-semibold text-zinc-100"
                    >
                      Execução &amp; Playground
                    </span>
                    <span className="shrink-0 rounded bg-zinc-800/80 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">
                      Idle
                    </span>
                  </span>
                  <span
                    title="Inferência e testes"
                    className="mt-0.5 block truncate text-[11px] text-zinc-400"
                  >
                    Inferência e testes
                  </span>
                </span>
              </span>
            </div>
          </div>

          {/* Telemetria do nó (placeholders honestos) */}
          <div
            title="Telemetria chega na fatia 4"
            className="glass-card space-y-3 rounded-xl border border-zinc-800/80 bg-zinc-950/60 p-3.5"
          >
            <div className="flex items-center justify-between">
              <span className="font-mono text-[10px] font-semibold uppercase tracking-caps text-zinc-400">
                Telemetria do Nó
              </span>
              <span className="font-mono text-[10px] text-zinc-500">—</span>
            </div>
            {TELEMETRY_ROWS.map((row) => (
              <div key={row.label}>
                <div className="mb-1 flex justify-between font-mono text-[10px] text-zinc-400">
                  <span>{row.label}</span>
                  <span title={row.value} className="text-zinc-200">
                    {row.value}
                  </span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                  <div className="h-full w-0 rounded-full bg-brand-500" />
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Rodapé */}
        <div className="flex shrink-0 items-center justify-between border-t border-zinc-800/80 bg-zinc-950/80 p-3">
          <span
            title="Fatia futura"
            aria-disabled="true"
            className="touch-target flex cursor-not-allowed items-center space-x-2 rounded-xl px-3 py-2 text-xs text-zinc-400 opacity-55"
          >
            <IconSettings />
            <span>Configurações</span>
          </span>
          <button
            type="button"
            onClick={logout}
            disabled={leaving}
            title={leaving ? "Saindo…" : "Sair da sessão"}
            aria-label="Sair da sessão"
            className="touch-target touch-manipulation flex min-w-[44px] items-center justify-center rounded-xl p-2 text-zinc-400 transition-colors hover:bg-rose-950/30 hover:text-rose-400 disabled:opacity-55"
          >
            <IconLogOut />
          </button>
        </div>
      </aside>
    </>
  );
}
