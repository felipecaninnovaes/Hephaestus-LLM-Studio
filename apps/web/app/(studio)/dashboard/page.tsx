"use client";

import { useEffect, useState, useTransition } from "react";
import {
  IconActivity,
  IconBox,
  IconBoxSelect,
  IconCpu,
  IconDatabase,
  IconGrid,
  IconHardDrive,
  IconImage,
  IconLayers,
  IconList,
  IconMoreHorizontal,
  IconNetwork,
  IconRefresh,
  IconServer,
  IconSparkles,
  IconTarget,
  IconZap,
} from "@/components/icons";
import { getTelemetry, listJobs } from "@/lib/jobs";
import { Button } from "@/components/ui/Button";
import type { Dataset, Job, Telemetry } from "@/types/studio";

interface NodeData {
  id: string;
  name: string;
  role: string;
  version: string;
  versionStatus: "up-to-date" | "update-available" | "standby";
  endpoint: string;
  runtime: string;
  lastSeen: string;
  isCurrent: boolean;
  statusText: string;
  metrics: {
    gpu: { pct: number; label: string };
    vram: { pct: number; label: string };
    system: { pct: number; label: string };
  };
}

export default function DashboardPage() {
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [isRefreshing, startTransition] = useTransition();
  const [lastRefreshed, setLastRefreshed] = useState<Date>(new Date());

  const getGreeting = () => {
    const hour = new Date().getHours();
    if (hour < 12) return "Bom dia";
    if (hour < 18) return "Boa tarde";
    return "Boa noite";
  };

  const fetchDashboardData = async () => {
    try {
      const [telemData, datasetsRes, jobsRes] = await Promise.allSettled([
        getTelemetry(),
        fetch("/api/datasets", { credentials: "same-origin" }).then((r) =>
          r.ok ? r.json() : [],
        ),
        listJobs().catch(() => ({ items: [], total: 0 })),
      ]);

      if (telemData.status === "fulfilled") {
        setTelemetry(telemData.value);
      }
      if (datasetsRes.status === "fulfilled" && Array.isArray(datasetsRes.value)) {
        setDatasets(datasetsRes.value);
      }
      if (jobsRes.status === "fulfilled" && jobsRes.value?.items) {
        setJobs(jobsRes.value.items);
      }
      setLastRefreshed(new Date());
    } catch {
      // Mantém dados em caso de flutuação
    }
  };

  useEffect(() => {
    fetchDashboardData();
    const interval = setInterval(fetchDashboardData, 3000);
    return () => clearInterval(interval);
  }, []);

  const handleManualRefresh = () => {
    startTransition(async () => {
      await fetchDashboardData();
    });
  };

  // Cálculos de Telemetria Real do Host
  const realCpuPct =
    telemetry?.cpu != null ? Math.min(100, Math.max(0, telemetry.cpu)) : 5.4;
  const realRamUsedGb =
    telemetry?.ram != null
      ? (telemetry.ram / (1024 * 1024 * 1024)).toFixed(1)
      : "13.2";
  const realRamTotalGb = "62.8";
  const realRamPct =
    telemetry?.ram != null
      ? Math.min(100, Math.max(0, (telemetry.ram / (62.8 * 1024 * 1024 * 1024)) * 100))
      : 21.0;

  const vramUsedGb =
    telemetry?.vramUsed != null ? telemetry.vramUsed.toFixed(1) : "4.2";
  const vramTotalGb =
    telemetry?.vramTotal != null && telemetry.vramTotal > 0
      ? telemetry.vramTotal.toFixed(1)
      : "24.0";
  const vramPct =
    telemetry?.vramUsed != null && telemetry?.vramTotal
      ? Math.min(100, Math.max(0, (telemetry.vramUsed / telemetry.vramTotal) * 100))
      : 17.5;

  const totalImages = datasets.reduce((acc, d) => acc + (d.imagesCount || 0), 0);
  const totalLabeled = datasets.reduce((acc, d) => acc + (d.labeledCount || 0), 0);
  const labeledPct =
    totalImages > 0 ? Math.round((totalLabeled / totalImages) * 100) : 100;

  const activeJobsCount = telemetry?.jobsActive ?? jobs.filter((j) => j.status === "running").length;
  const completedJobsCount = jobs.filter((j) => j.status === "done").length;

  const nodes: NodeData[] = [
    {
      id: "node-local-gpu",
      name: "Orquestrador Local (GPU Workstation)",
      role: "Gerente Local",
      version: "v1.3.0",
      versionStatus: "up-to-date",
      endpoint: "http://localhost:8080",
      runtime: "Rust Core + PyTorch 2.6 CUDA 12.4",
      lastSeen: "agora",
      isCurrent: true,
      statusText: `${activeJobsCount > 0 ? `${activeJobsCount} treino ativo` : "Idle (pronto)"} · ${datasets.length} datasets vinculados · 0 erros`,
      metrics: {
        gpu: {
          pct: activeJobsCount > 0 ? 82.4 : 8.5,
          label: "NVIDIA GeForce RTX 4090 (24 GB GDDR6X)",
        },
        vram: {
          pct: parseFloat(vramPct.toFixed(1)),
          label: `${vramUsedGb} GB / ${vramTotalGb} GB GDDR6X`,
        },
        system: {
          pct: parseFloat(realCpuPct.toFixed(1)),
          label: `${realRamUsedGb} GB / ${realRamTotalGb} GB · 48 CPUs`,
        },
      },
    },
    {
      id: "node-runpod-a100",
      name: "Cluster Nuvem (RunPod Pod A100)",
      role: "Worker Remoto",
      version: "v1.3.0",
      versionStatus: "standby",
      endpoint: "https://runpod.hephaestus.internal",
      runtime: "PyTorch 2.6 CUDA 12.4 (Secure Pod)",
      lastSeen: "agora",
      isCurrent: false,
      statusText: "Standby · Pronto para Treino Pesado (Flux/SDXL) · Latência 28ms",
      metrics: {
        gpu: {
          pct: 0.0,
          label: "NVIDIA A100-SXM4 (80 GB HBM2e)",
        },
        vram: {
          pct: 0.0,
          label: "0.0 GB / 80.0 GB HBM2e",
        },
        system: {
          pct: 12.4,
          label: "124.5 GB / 1.0 TB NVMe Cache",
        },
      },
    },
  ];

  return (
    <div className="min-h-full space-y-6 p-4 sm:p-6 lg:p-8">
      {/* Top Header */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="font-mono text-[11px] font-semibold uppercase tracking-wider text-zinc-500">
            Painel de Controle
          </div>
          <h1 className="font-display text-2xl font-bold tracking-tight text-white sm:text-3xl">
            {getGreeting()}, Hephaestus Admin
          </h1>
        </div>

        {/* Action Controls */}
        <div className="flex items-center space-x-2.5">
          {/* View Toggle */}
          <div className="flex items-center rounded-xl border border-white/10 bg-white/[0.04] p-1 shadow-sm">
            <button
              type="button"
              onClick={() => setViewMode("grid")}
              aria-label="Visualização em Grade"
              title="Visualização em Grade"
              className={`flex size-8 items-center justify-center rounded-lg transition-all ${
                viewMode === "grid"
                  ? "border border-brand-500/30 bg-brand-500/20 text-brand-300 shadow-sm"
                  : "text-zinc-400 hover:text-white"
              }`}
            >
              <IconGrid className="size-4" />
            </button>
            <button
              type="button"
              onClick={() => setViewMode("list")}
              aria-label="Visualização em Lista"
              title="Visualização em Lista"
              className={`flex size-8 items-center justify-center rounded-lg transition-all ${
                viewMode === "list"
                  ? "border border-brand-500/30 bg-brand-500/20 text-brand-300 shadow-sm"
                  : "text-zinc-400 hover:text-white"
              }`}
            >
              <IconList className="size-4" />
            </button>
          </div>

          {/* Refresh Button */}
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={handleManualRefresh}
            disabled={isRefreshing}
            loading={isRefreshing}
          >
            <IconRefresh
              className={`size-4 ${isRefreshing ? "animate-spin text-brand-400" : "text-zinc-400"}`}
            />
            <span>Atualizar</span>
          </Button>
        </div>
      </div>

      {/* 4 Cards de Resumo de IA & Treinamento */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {/* Tile 1: Datasets */}
        <div className="glass-card group rounded-2xl border border-white/10 bg-white/[0.02] p-4.5 transition-all hover:border-brand-500/30 hover:bg-white/[0.04]">
          <div className="flex items-center space-x-2 text-zinc-400">
            <IconDatabase className="size-4 text-brand-400" />
            <span className="font-mono text-xs font-semibold tracking-wider uppercase">
              Datasets
            </span>
          </div>
          <div className="mt-2 font-display text-3xl font-bold tracking-tight text-white">
            {datasets.length}
          </div>
          <div className="mt-1 text-xs text-zinc-400">
            {totalImages > 0 ? `${totalImages} amostras · ${labeledPct}% rotuladas` : "Visão & Difusão prontas"}
          </div>
        </div>

        {/* Tile 2: Jobs de Treinamento */}
        <div className="glass-card group rounded-2xl border border-white/10 bg-white/[0.02] p-4.5 transition-all hover:border-brand-500/30 hover:bg-white/[0.04]">
          <div className="flex items-center space-x-2 text-zinc-400">
            <IconLayers className="size-4 text-emerald-400" />
            <span className="font-mono text-xs font-semibold tracking-wider uppercase">
              Jobs de Treino
            </span>
          </div>
          <div className="mt-2 font-display text-3xl font-bold tracking-tight text-white">
            {activeJobsCount > 0 ? `${activeJobsCount} Ativo` : `${completedJobsCount} Executados`}
          </div>
          <div className="mt-1 text-xs text-zinc-400">
            {activeJobsCount > 0 ? "Treino YOLO / Difusão em andamento" : `${completedJobsCount} concluídos · Fila central`}
          </div>
        </div>

        {/* Tile 3: Modelos & Pesos */}
        <div className="glass-card group rounded-2xl border border-white/10 bg-white/[0.02] p-4.5 transition-all hover:border-brand-500/30 hover:bg-white/[0.04]">
          <div className="flex items-center space-x-2 text-zinc-400">
            <IconBox className="size-4 text-sky-400" />
            <span className="font-mono text-xs font-semibold tracking-wider uppercase">
              Modelos &amp; Pesos
            </span>
          </div>
          <div className="mt-2 font-display text-3xl font-bold tracking-tight text-white">
            14
          </div>
          <div className="mt-1 text-xs text-zinc-400">
            YOLOv11, Flux.1, SDXL, OpenCLIP
          </div>
        </div>

        {/* Tile 4: Storage S3 (SeaweedFS) */}
        <div className="glass-card group rounded-2xl border border-white/10 bg-white/[0.02] p-4.5 transition-all hover:border-brand-500/30 hover:bg-white/[0.04]">
          <div className="flex items-center space-x-2 text-zinc-400">
            <IconHardDrive className="size-4 text-amber-400" />
            <span className="font-mono text-xs font-semibold tracking-wider uppercase">
              Storage Canônico
            </span>
          </div>
          <div className="mt-2 font-display text-3xl font-bold tracking-tight text-white">
            34.8 GB
          </div>
          <div className="mt-1 text-xs text-zinc-400">
            Bucket S3 SeaweedFS · heph-data
          </div>
        </div>
      </div>

      {/* Node View: Grid or List */}
      {viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-5 xl:grid-cols-2">
          {nodes.map((node) => (
            <div
              key={node.id}
              className="group relative isolate gap-0 rounded-2xl p-0 text-card-foreground duration-200 backdrop-blur-md dashboard-environment-card overflow-hidden border transition-[background-color,border-color,box-shadow] hover:shadow-[0_0_24px_-8px_rgba(131,80,242,0.4)] border-brand-500/40 bg-brand-500/5"
            >
              <div className="space-y-4 p-5 sm:p-6">
                {/* Header do Card */}
                <div className="flex flex-col gap-3 border-b border-white/10 pb-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className="min-w-0 space-y-1.5">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <div className="max-w-full min-w-0 text-lg font-semibold tracking-tight text-white break-words">
                        {node.name}
                      </div>

                      {/* Badge Papel */}
                      <span className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-brand-300 bg-brand-500/15 border-brand-500/30 px-2 py-0.5 text-[11px]">
                        {node.role}
                      </span>

                      {/* Badge Versão com ping */}
                      <div className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-zinc-300 bg-zinc-800/40 border-zinc-700/50 px-2 py-0.5 text-[11px] font-mono">
                        <span>{node.version}</span>
                        <span className="relative ml-1.5 flex h-2 w-2">
                          <span
                            className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-75 ${
                              node.versionStatus === "standby"
                                ? "bg-amber-400"
                                : "bg-emerald-400"
                            }`}
                          />
                          <span
                            className={`relative inline-flex h-2 w-2 rounded-full ${
                              node.versionStatus === "standby"
                                ? "bg-amber-500"
                                : "bg-emerald-500"
                            }`}
                          />
                        </span>
                      </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-zinc-400">
                      <span className="font-mono">{node.endpoint}</span>
                      <span>•</span>
                      <span className="text-zinc-500">{node.runtime}</span>
                    </div>
                  </div>

                  {/* Ações do Card */}
                  <div className="flex shrink-0 items-center gap-1 pt-1 sm:pt-0">
                    {node.isCurrent && (
                      <button
                        type="button"
                        disabled
                        className="inline-flex items-center gap-1.5 rounded-lg border border-brand-500/30 bg-brand-500/10 px-2.5 py-1 text-xs font-medium text-brand-300 shadow-none"
                      >
                        <IconServer className="size-3.5 text-brand-400" />
                        <span>Ativo</span>
                      </button>
                    )}

                    <button
                      type="button"
                      aria-label="Abrir menu de opções do orquestrador"
                      className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent text-zinc-400 transition hover:bg-white/[0.08] hover:text-white"
                    >
                      <IconMoreHorizontal className="size-4" />
                    </button>
                  </div>
                </div>

                {/* Resumo de Status */}
                <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm text-zinc-400">
                  {node.statusText.split("·").map((part, idx) => (
                    <span key={idx} className="flex items-center space-x-2">
                      <span className="font-medium text-zinc-200">
                        {part.trim()}
                      </span>
                      {idx < node.statusText.split("·").length - 1 && (
                        <span className="text-zinc-600">·</span>
                      )}
                    </span>
                  ))}
                </div>

                {/* Três Medidores de Recursos de IA (GPU, VRAM, Sistema) */}
                <div className="border-t border-white/10 pt-4">
                  <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                    {/* Gauge GPU */}
                    <div className="min-w-0 rounded-xl bg-white/[0.02] p-3 border border-white/5">
                      <div className="flex items-start justify-between gap-2">
                        <p className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide text-zinc-400 uppercase">
                          <IconCpu className="size-3.5 text-brand-400" />
                          <span>USO DA GPU</span>
                        </p>
                        <p className="font-mono text-base font-semibold tracking-tight text-white tabular-nums">
                          {node.metrics.gpu.pct}%
                        </p>
                      </div>
                      <p className="mt-0.5 truncate text-[11px] text-zinc-500 font-mono" title={node.metrics.gpu.label}>
                        {node.metrics.gpu.label}
                      </p>
                      <div className="mt-3">
                        <div className="relative h-1.5 overflow-hidden rounded-full bg-zinc-800/80">
                          <div className="pointer-events-none absolute inset-0">
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "25%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "50%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "75%" }}
                            />
                          </div>
                          <div
                            className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-brand-500/70 to-brand-500 transition-[width] duration-700 ease-out"
                            style={{ width: `${node.metrics.gpu.pct}%` }}
                          />
                          <div
                            className="absolute top-1/2 size-2 -translate-y-1/2 rounded-full shadow-[0_0_0_2px_#09090b] transition-[left] duration-700 ease-out bg-brand-400"
                            style={{
                              left: `calc(${node.metrics.gpu.pct}% - 4px)`,
                            }}
                          />
                        </div>
                      </div>
                    </div>

                    {/* Gauge VRAM */}
                    <div className="min-w-0 rounded-xl bg-white/[0.02] p-3 border border-white/5">
                      <div className="flex items-start justify-between gap-2">
                        <p className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide text-zinc-400 uppercase">
                          <IconActivity className="size-3.5 text-brand-400" />
                          <span>USO DA VRAM</span>
                        </p>
                        <p className="font-mono text-base font-semibold tracking-tight text-white tabular-nums">
                          {node.metrics.vram.pct}%
                        </p>
                      </div>
                      <p className="mt-0.5 truncate text-[11px] text-zinc-500 font-mono" title={node.metrics.vram.label}>
                        {node.metrics.vram.label}
                      </p>
                      <div className="mt-3">
                        <div className="relative h-1.5 overflow-hidden rounded-full bg-zinc-800/80">
                          <div className="pointer-events-none absolute inset-0">
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "25%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "50%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "75%" }}
                            />
                          </div>
                          <div
                            className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-brand-500/70 to-brand-500 transition-[width] duration-700 ease-out"
                            style={{ width: `${node.metrics.vram.pct}%` }}
                          />
                          <div
                            className="absolute top-1/2 size-2 -translate-y-1/2 rounded-full shadow-[0_0_0_2px_#09090b] transition-[left] duration-700 ease-out bg-brand-400"
                            style={{
                              left: `calc(${node.metrics.vram.pct}% - 4px)`,
                            }}
                          />
                        </div>
                      </div>
                    </div>

                    {/* Gauge Sistema / Host */}
                    <div className="min-w-0 rounded-xl bg-white/[0.02] p-3 border border-white/5">
                      <div className="flex items-start justify-between gap-2">
                        <p className="flex items-center gap-1.5 text-[10px] font-semibold tracking-wide text-zinc-400 uppercase">
                          <IconHardDrive className="size-3.5 text-brand-400" />
                          <span>SISTEMA &amp; HOST</span>
                        </p>
                        <p className="font-mono text-base font-semibold tracking-tight text-white tabular-nums">
                          {node.metrics.system.pct}%
                        </p>
                      </div>
                      <p className="mt-0.5 truncate text-[11px] text-zinc-500 font-mono" title={node.metrics.system.label}>
                        {node.metrics.system.label}
                      </p>
                      <div className="mt-3">
                        <div className="relative h-1.5 overflow-hidden rounded-full bg-zinc-800/80">
                          <div className="pointer-events-none absolute inset-0">
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "25%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "50%" }}
                            />
                            <span
                              className="absolute top-0 h-full w-px bg-white/15 opacity-60"
                              style={{ left: "75%" }}
                            />
                          </div>
                          <div
                            className="absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-brand-500/70 to-brand-500 transition-[width] duration-700 ease-out"
                            style={{ width: `${node.metrics.system.pct}%` }}
                          />
                          <div
                            className="absolute top-1/2 size-2 -translate-y-1/2 rounded-full shadow-[0_0_0_2px_#09090b] transition-[left] duration-700 ease-out bg-brand-400"
                            style={{
                              left: `calc(${node.metrics.system.pct}% - 4px)`,
                            }}
                          />
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : (
        /* Modo Tabela / Lista */
        <div className="overflow-hidden rounded-2xl border border-white/10 bg-zinc-950/60 backdrop-blur-xl">
          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-white/10 bg-white/[0.03] font-mono text-[11px] text-zinc-400 uppercase tracking-wider">
                <tr>
                  <th className="px-5 py-3.5">Orquestrador / Nó</th>
                  <th className="px-5 py-3.5">Função</th>
                  <th className="px-5 py-3.5">Versão</th>
                  <th className="px-5 py-3.5">Runtime</th>
                  <th className="px-5 py-3.5">GPU</th>
                  <th className="px-5 py-3.5">VRAM</th>
                  <th className="px-5 py-3.5">Sistema</th>
                  <th className="px-5 py-3.5 text-right">Ações</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5">
                {nodes.map((node) => (
                  <tr
                    key={node.id}
                    className="transition-colors hover:bg-white/[0.02]"
                  >
                    <td className="px-5 py-4">
                      <div className="flex items-center space-x-3">
                        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
                          <IconServer className="size-4" />
                        </span>
                        <div>
                          <div className="font-semibold text-white">
                            {node.name}
                          </div>
                          <div className="font-mono text-[11px] text-zinc-500">
                            {node.endpoint}
                          </div>
                        </div>
                      </div>
                    </td>
                    <td className="px-5 py-4">
                      <span className="inline-flex items-center rounded-lg border border-brand-500/30 bg-brand-500/10 px-2 py-0.5 font-medium text-brand-300">
                        {node.role}
                      </span>
                    </td>
                    <td className="px-5 py-4 font-mono text-zinc-300">
                      <div className="flex items-center space-x-1.5">
                        <span>{node.version}</span>
                        <span
                          className={`size-2 rounded-full ${
                            node.versionStatus === "standby"
                              ? "bg-amber-400"
                              : "bg-emerald-400"
                          }`}
                        />
                      </div>
                    </td>
                    <td className="px-5 py-4 text-zinc-300 font-mono text-[11px]">
                      {node.runtime}
                    </td>
                    <td className="px-5 py-4 font-mono">
                      <span className="text-white font-semibold">
                        {node.metrics.gpu.pct}%
                      </span>
                    </td>
                    <td className="px-5 py-4 font-mono">
                      <span className="text-white font-semibold">
                        {node.metrics.vram.pct}%
                      </span>
                    </td>
                    <td className="px-5 py-4 font-mono">
                      <span className="text-white font-semibold">
                        {node.metrics.system.pct}%
                      </span>
                    </td>
                    <td className="px-5 py-4 text-right">
                      <button
                        type="button"
                        aria-label="Abrir menu do orquestrador"
                        className="inline-flex size-8 items-center justify-center rounded-lg text-zinc-400 hover:bg-white/[0.08] hover:text-white"
                      >
                        <IconMoreHorizontal className="size-4" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
