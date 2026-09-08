"use client";

import { useEffect, useState } from "react";
import { useRouter, usePathname } from "next/navigation";
import {
  IconCpu,
  IconDatabase,
  IconLayers,
  IconLogOut,
  IconSettings,
  IconTarget,
  IconX,
} from "@/components/icons";
import { getTelemetry } from "@/lib/jobs";
import type { Telemetry } from "@/types/studio";

interface SidebarProps {
  open: boolean;
  onClose: () => void;
}

const TELEMETRY_POLL_MS = 3000;

export default function Sidebar({ open, onClose }: SidebarProps) {
  const router = useRouter();
  const pathname = usePathname();
  const [leaving, setLeaving] = useState(false);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);

  const isDatasetsActive = pathname?.startsWith("/datasets") ?? false;
  const isJobsActive = pathname === "/jobs" || (pathname?.startsWith("/jobs") ?? false);

  // Telemetry polling
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setInterval> | null = null;

    async function fetchTelemetry() {
      try {
        const data = await getTelemetry();
        if (active) setTelemetry(data);
      } catch {
        // Mantém estado anterior se falhar
      }
    }

    fetchTelemetry();
    timer = setInterval(fetchTelemetry, TELEMETRY_POLL_MS);
    return () => {
      active = false;
      if (timer) clearInterval(timer);
    };
  }, []);

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

  const cpuPct = telemetry?.cpu != null ? Math.min(100, Math.max(0, telemetry.cpu)) : null;
  const vramPct =
    telemetry?.measured && telemetry.vramUsed != null && telemetry.vramTotal != null && telemetry.vramTotal > 0
      ? Math.min(100, Math.max(0, (telemetry.vramUsed / telemetry.vramTotal) * 100))
      : null;

  function formatRam(val: number | null): string {
    if (val == null) return "—";
    return `${(val / (1024 * 1024 * 1024)).toFixed(1)} GB`;
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
            className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 lg:hidden"
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
              {/* Dados & Anotação */}
              <a
                href="/datasets"
                onClick={onClose}
                className={`relative flex w-full items-start space-x-3 overflow-hidden rounded-xl border p-2.5 text-left text-white ${
                  isDatasetsActive
                    ? "border-brand-500/30 bg-zinc-900/90 shadow-sm"
                    : "border-transparent transition-colors hover:bg-white/[0.06]"
                }`}
              >
                {isDatasetsActive && <span className="absolute top-1/2 left-0 h-6 w-1 -translate-y-1/2 rounded-r-full bg-brand-500" />}
                <span className={`shrink-0 rounded-lg border p-2 ${
                  isDatasetsActive
                    ? "border-brand-500/30 bg-brand-500/15 text-brand-400"
                    : "border-white/10 bg-zinc-900 text-zinc-300"
                }`}>
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
              </a>

              {/* Forja & Treinamento — HABILITADO */}
              <a
                href="/jobs"
                onClick={onClose}
                className={`relative flex w-full items-start space-x-3 overflow-hidden rounded-xl border p-2.5 text-left ${
                  isJobsActive
                    ? "border-brand-500/30 bg-zinc-900/90 text-white shadow-sm"
                    : "border-transparent text-zinc-200 transition-colors hover:bg-white/[0.06] hover:text-white"
                }`}
              >
                {isJobsActive && <span className="absolute top-1/2 left-0 h-6 w-1 -translate-y-1/2 rounded-r-full bg-brand-500" />}
                <span className={`shrink-0 rounded-lg border p-2 ${
                  isJobsActive
                    ? "border-brand-500/30 bg-brand-500/15 text-brand-400"
                    : "border-white/10 bg-zinc-900 text-zinc-300"
                }`}>
                  <IconLayers />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center justify-between gap-2">
                    <span
                      title="Forja & Treinamento"
                      className="min-w-0 flex-1 truncate text-xs font-semibold"
                    >
                      Forja &amp; Treinamento
                    </span>
                    {telemetry != null && telemetry.jobsActive > 0 && (
                      <span className="shrink-0 rounded bg-brand-500/20 px-1.5 py-0.5 font-mono text-[10px] text-brand-300">
                        {telemetry.jobsActive} active
                      </span>
                    )}
                  </span>
                  <span
                    title="Treino YOLO ativo"
                    className="mt-0.5 block truncate text-[11px] text-zinc-400"
                  >
                    Treino YOLO ativo
                  </span>
                </span>
              </a>

              {/* Desabilitados honestos */}
              <span
                title="Disponível em fatia futura"
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

          {/* Telemetria do nó (REAL) */}
          <div className="glass-card space-y-3 rounded-xl border border-zinc-800/80 bg-zinc-950/60 p-3.5">
            <div className="flex items-center justify-between">
              <span className="font-mono text-[10px] font-semibold uppercase tracking-caps text-zinc-400">
                Telemetria do Nó
              </span>
              {telemetry?.jobsActive != null && telemetry.jobsActive > 0 && (
                <span className="font-mono text-[10px] text-brand-300">
                  {telemetry.jobsActive} job{telemetry.jobsActive > 1 ? "s" : ""}
                </span>
              )}
            </div>

            {/* VRAM */}
            <div>
              <div className="mb-1 flex justify-between font-mono text-[10px] text-zinc-400">
                <span>VRAM</span>
                <span className="text-zinc-200">
                  {!telemetry?.measured
                    ? "sem GPU (mock)"
                    : vramPct != null
                      ? `${telemetry!.vramUsed!.toFixed(1)} / ${telemetry!.vramTotal!.toFixed(1)} GB`
                      : "—"}
                </span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-brand-500 transition-all duration-500"
                  style={{ width: vramPct != null ? `${vramPct}%` : "0%" }}
                />
              </div>
            </div>

            {/* CPU */}
            <div>
              <div className="mb-1 flex justify-between font-mono text-[10px] text-zinc-400">
                <span>CPU</span>
                <span className="text-zinc-200">
                  {cpuPct != null ? `${cpuPct.toFixed(1)}%` : "—"}
                </span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-brand-500 transition-all duration-500"
                  style={{ width: cpuPct != null ? `${cpuPct}%` : "0%" }}
                />
              </div>
            </div>

            {/* RAM — Wire only sends bytes, not total, so no percentage bar is shown */}
            <div>
              <div className="mb-1 flex justify-between font-mono text-[10px] text-zinc-400">
                <span>RAM</span>
                <span className="text-zinc-200">
                  {telemetry?.ram != null ? formatRam(telemetry.ram) : "—"}
                </span>
              </div>
            </div>

            {/* GPUs info */}
            {telemetry?.gpus && telemetry.gpus.length > 0 && (
              <div className="flex items-center gap-2 border-t border-zinc-800/60 pt-2">
                <span className="font-mono text-[10px] text-zinc-500">GPU:</span>
                <span className="truncate font-mono text-[10px] text-zinc-300" title={telemetry.gpus.join(", ")}>
                  {telemetry.gpus.join(", ")}
                </span>
              </div>
            )}
          </div>
        </div>

        {/* Rodapé */}
        <div className="flex shrink-0 items-center justify-between border-t border-zinc-800/80 bg-zinc-950/80 p-3">
          <span
            title="Disponível em fatia futura"
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
            className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-300 transition hover:bg-white/[0.06] hover:text-rose-300 active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconLogOut />
          </button>
        </div>
      </aside>
    </>
  );
}
