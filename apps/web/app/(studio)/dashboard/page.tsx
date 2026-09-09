"use client";

import { useEffect, useState, useTransition } from "react";
import {
  IconCpu,
  IconDatabase,
  IconGrid,
  IconHardDrive,
  IconImage,
  IconLayers,
  IconList,
  IconRefresh,
  IconServer,
} from "@/components/icons";
import { getTelemetry, listJobs } from "@/lib/jobs";
import {
  listOrchestrators,
  listModels,
  getStorageUsage,
  type Orchestrator,
  type ModelWeight,
  type StorageUsage,
} from "@/lib/monitoring";
import { Button, SegmentedControl, StatCard, GlassCard } from "@/components/ui";
import { MetricTile } from "@/components/ui/MetricTile";
import { TruncatedText } from "@/components/ui/TruncatedText";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import type { Dataset, Job, Telemetry } from "@/types/studio";

export default function DashboardPage() {
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [orchestrators, setOrchestrators] = useState<Orchestrator[]>([]);
  const [models, setModels] = useState<ModelWeight[]>([]);
  const [storageUsage, setStorageUsage] = useState<StorageUsage | null>(null);
  const [isRefreshing, startTransition] = useTransition();
  const [lastRefreshed, setLastRefreshed] = useState<Date>(new Date());

  const fetchDashboardData = async () => {
    if (typeof document !== "undefined" && document.visibilityState === "hidden") {
      return;
    }
    try {
      const [telemData, datasetsRes, jobsRes, orchData, modelsData, storageData] =
        await Promise.allSettled([
          getTelemetry(),
          fetch("/api/datasets", { credentials: "same-origin" }).then((r) =>
            r.ok ? r.json() : [],
          ),
          listJobs().catch(() => ({ items: [], total: 0 })),
          listOrchestrators().catch(() => ({ items: [] })),
          listModels().catch(() => ({ items: [] })),
          getStorageUsage().catch(() => null),
        ]);

      if (telemData.status === "fulfilled") setTelemetry(telemData.value);
      if (datasetsRes.status === "fulfilled" && Array.isArray(datasetsRes.value))
        setDatasets(datasetsRes.value);
      if (jobsRes.status === "fulfilled" && jobsRes.value?.items)
        setJobs(jobsRes.value.items);
      if (orchData.status === "fulfilled") setOrchestrators(orchData.value.items);
      if (modelsData.status === "fulfilled") setModels(modelsData.value.items);
      if (storageData.status === "fulfilled") setStorageUsage(storageData.value);

      setLastRefreshed(new Date());
    } catch {
      // Mantém dados em caso de flutuação
    }
  };

  useEffect(() => {
    fetchDashboardData();
    const interval = setInterval(fetchDashboardData, 3000);
    const handleVisibilityChange = () => {
      if (typeof document !== "undefined" && document.visibilityState === "visible") {
        fetchDashboardData();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, []);

  const handleManualRefresh = () => {
    startTransition(async () => {
      await fetchDashboardData();
    });
  };

  /* ── Derived metrics (honest — no invented numbers) ──────── */

  const node = orchestrators.length === 1 ? orchestrators[0] : null;
  const hasGpu = (telemetry?.gpus?.length ?? 0) > 0 && telemetry?.vramTotal != null;

  const cpuPct = telemetry?.cpu != null ? Math.min(100, Math.max(0, telemetry.cpu)) : null;

  const ramUsedGb =
    telemetry?.ram != null ? (telemetry.ram / (1024 * 1024 * 1024)).toFixed(1) : null;
  const ramTotalGb =
    telemetry?.ramTotal != null ? (telemetry.ramTotal / (1024 * 1024 * 1024)).toFixed(1) : null;
  const ramPct =
    telemetry?.ram != null && telemetry?.ramTotal != null && telemetry.ramTotal > 0
      ? Math.min(100, Math.max(0, (telemetry.ram / telemetry.ramTotal) * 100))
      : null;

  const vramUsedGb = telemetry?.vramUsed != null ? telemetry.vramUsed.toFixed(1) : null;
  const vramTotalGb = telemetry?.vramTotal != null ? telemetry.vramTotal.toFixed(1) : null;
  const vramPct =
    telemetry?.vramUsed != null && telemetry?.vramTotal != null && telemetry.vramTotal > 0
      ? Math.min(100, Math.max(0, (telemetry.vramUsed / telemetry.vramTotal) * 100))
      : null;

  const gpuLabel = hasGpu ? telemetry!.gpus[0] : null;

  const totalImages = datasets.reduce((acc, d) => acc + (d.imagesCount || 0), 0);
  const totalLabeled = datasets.reduce((acc, d) => acc + (d.labeledCount || 0), 0);
  const labeledPct =
    totalImages > 0 ? Math.round((totalLabeled / totalImages) * 100) : 100;

  const activeJobsCount = telemetry?.jobsActive ?? jobs.filter((j) => j.status === "running").length;
  const completedJobsCount = jobs.filter((j) => j.status === "done").length;

  const fmt = (v: number | null | undefined, decimals = 1): string =>
    v != null ? v.toFixed(decimals) : "—";

  return (
    <div className="min-h-full space-y-6 p-4 sm:p-6 lg:p-8">
      {/* Top Header */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400">
            Painel de Controle
          </div>
          <h1 className="font-display text-2xl font-bold tracking-tight text-white sm:text-3xl">
            Operador local
          </h1>
        </div>

        {/* Action Controls */}
        <div className="flex items-center space-x-2.5">
          <SegmentedControl<"grid" | "list">
            ariaLabel="Modo de visualização"
            value={viewMode}
            onChange={setViewMode}
            options={[
              {
                id: "grid",
                icon: <IconGrid className="h-4 w-4" />,
                title: "Visualização em Grade",
                ariaLabel: "Visualização em Grade",
              },
              {
                id: "list",
                icon: <IconList className="h-4 w-4" />,
                title: "Visualização em Lista",
                ariaLabel: "Visualização em Lista",
              },
            ]}
          />

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

      {/* 4 Cards de Resumo */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard
          label="Datasets"
          value={datasets.length}
          subtext={totalImages > 0 ? `${totalImages} amostras · ${labeledPct}% rotuladas` : "Sem dados de mídia"}
          icon={<IconDatabase className="size-4" />}
          iconColor="text-brand-400"
        />
        <StatCard
          label="Jobs de Treino"
          value={activeJobsCount > 0 ? `${activeJobsCount} Ativo` : `${completedJobsCount} Executados`}
          subtext={activeJobsCount > 0 ? "Treino em andamento" : `${completedJobsCount} concluídos`}
          icon={<IconLayers className="size-4" />}
          iconColor="text-[#34d399]"
        />
        <StatCard
          label="Modelos & Pesos"
          value={models.length}
          subtext={models.length > 0 ? "pesos de treinos (mock)" : "Sem pesos gerados"}
          icon={<IconImage className="size-4" />}
          iconColor="text-[#06b6d4]"
        />
        <StatCard
          label="Storage Canônico"
          value={storageUsage ? formatBytes(storageUsage.totalBytes) : "—"}
          subtext="rastreados pelo banco · Bucket S3"
          icon={<IconHardDrive className="size-4" />}
          iconColor="text-[#f59e0b]"
        />
      </div>

      {/* Node View: Grid or List */}
      {viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-5 xl:grid-cols-2">
          {node && (
            <GlassCard className="p-0 overflow-hidden">
              <div className="space-y-4 p-5 sm:p-6">
                {/* Header do Card */}
                <div className="flex flex-col gap-3 border-b border-white/10 pb-4 sm:flex-row sm:items-start sm:justify-between">
                  <div className="min-w-0 space-y-1.5">
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <div className="max-w-full min-w-0 text-lg font-semibold tracking-tight text-white break-words">
                        <TruncatedText text={node.name} as="span" />
                      </div>

                      {/* Badge Kind */}
                      <span className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-brand-300 bg-brand-500/15 border-brand-500/30 backdrop-blur-sm px-2 py-0.5 text-[11px]">
                        {node.kind === "local" ? "Local" : "Remoto"}
                      </span>

                      {/* Status badge */}
                      <div className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-zinc-300 bg-zinc-800/40 border-zinc-700/50 backdrop-blur-sm px-2 py-0.5 text-[11px] font-mono">
                        <span>{node.status}</span>
                        <span className="relative ml-1.5 flex h-2 w-2">
                          <span
                            className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-75 motion-reduce:animate-none ${
                              node.status === "online" ? "bg-[#34d399]" : "bg-[#f59e0b]"
                            }`}
                          />
                          <span
                            className={`relative inline-flex h-2 w-2 rounded-full ${
                              node.status === "online" ? "bg-[#34d399]" : "bg-[#f59e0b]"
                            }`}
                          />
                        </span>
                      </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-zinc-400">
                      <TruncatedText
                        text={node.endpoint}
                        className="font-mono"
                        as="span"
                      />
                      {node.lastHeartbeat && (
                        <>
                          <span>·</span>
                          <span className="font-mono">
                            visto há {formatRelativeTime(node.lastHeartbeat)}
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                </div>

                {/* Status derivado de dados reais */}
                <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm text-zinc-400">
                  <span className="font-medium text-zinc-200">
                    {activeJobsCount > 0 ? `${activeJobsCount} treino ativo` : "Idle (pronto)"}
                  </span>
                  <span className="text-zinc-500">·</span>
                  <span className="font-medium text-zinc-200">
                    {datasets.length} datasets vinculados
                  </span>
                </div>

                {/* Medidores de Recursos — só quando há 1 nó (D1) */}
                {orchestrators.length === 1 && (
                  <div className="border-t border-white/10 pt-4">
                    <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                      {/* GPU */}
                      <MetricTile
                        label="USO DA GPU"
                        value={gpuLabel ? `${fmt(vramPct)}%` : "sem GPU (mock)"}
                        highlightColor={gpuLabel ? "brand" : "default"}
                        subtext={gpuLabel ?? "Nenhuma GPU detectada"}
                      />

                      {/* VRAM */}
                      <MetricTile
                        label="USO DA VRAM"
                        value={
                          vramUsedGb && vramTotalGb
                            ? `${vramUsedGb} / ${vramTotalGb} GB`
                            : "—"
                        }
                        highlightColor="cyan"
                        subtext={vramPct != null ? `${vramPct.toFixed(1)}%` : "medido: —"}
                      />

                      {/* Sistema & Host */}
                      <MetricTile
                        label="SISTEMA & HOST"
                        value={cpuPct != null ? `${fmt(cpuPct)}%` : "—"}
                        highlightColor={cpuPct != null ? "default" : "default"}
                        subtext={
                          ramUsedGb && ramTotalGb
                            ? `${ramUsedGb} / ${ramTotalGb} GB RAM`
                            : "RAM: —"
                        }
                      />
                    </div>
                  </div>
                )}

                {/* Aviso quando há mais de 1 nó */}
                {orchestrators.length > 1 && (
                  <div className="border-t border-white/10 pt-4 text-xs text-zinc-400">
                    Telemetria de hardware indisponível (mais de 1 orquestrador registrado).
                  </div>
                )}
              </div>
            </GlassCard>
          )}

          {/* Sem nó registrado */}
          {!node && (
            <GlassCard className="p-5 text-center text-zinc-400 text-sm">
              Nenhum orquestrador registrado no sistema.
            </GlassCard>
          )}
        </div>
      ) : (
        /* Modo Tabela / Lista */
        <div className="glass-card overflow-hidden rounded-2xl shadow-2xl">
          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-white/10 bg-white/[0.03] font-mono text-[11px] text-zinc-400 uppercase tracking-[0.08em]">
                <tr>
                  <th className="px-5 py-3.5">Orquestrador</th>
                  <th className="px-5 py-3.5">Status</th>
                  <th className="px-5 py-3.5">Último Heartbeat</th>
                  <th className="px-5 py-3.5">Endpoint</th>
                  {orchestrators.length === 1 && (
                    <>
                      <th className="px-5 py-3.5">GPU</th>
                      <th className="px-5 py-3.5">VRAM</th>
                      <th className="px-5 py-3.5">CPU</th>
                    </>
                  )}
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5">
                {orchestrators.map((orch) => (
                  <tr
                    key={orch.id}
                    className="transition-colors hover:bg-white/[0.02]"
                  >
                    <td className="px-5 py-4">
                      <div className="flex items-center space-x-3">
                        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400">
                          <IconServer className="size-4" />
                        </span>
                        <div>
                          <div className="font-semibold text-white">
                            {orch.name}
                          </div>
                          <div className="font-mono text-[11px] text-zinc-400">
                            {orch.endpoint}
                          </div>
                        </div>
                      </div>
                    </td>
                    <td className="px-5 py-4">
                      <span className="inline-flex items-center rounded-lg border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm px-2 py-0.5 font-medium text-brand-300">
                        {orch.status}
                      </span>
                    </td>
                    <td className="px-5 py-4 font-mono text-zinc-300">
                      {orch.lastHeartbeat ? formatRelativeTime(orch.lastHeartbeat) : "—"}
                    </td>
                    <td className="px-5 py-4 text-zinc-300 font-mono text-[11px]">
                      <TruncatedText text={orch.endpoint} as="span" />
                    </td>
                    {orchestrators.length === 1 && (
                      <>
                        <td className="px-5 py-4 font-mono">
                          <span className="text-white font-semibold">
                            {gpuLabel ? `${fmt(vramPct)}%` : "—"}
                          </span>
                        </td>
                        <td className="px-5 py-4 font-mono">
                          <span className="text-white font-semibold">
                            {vramUsedGb && vramTotalGb
                              ? `${vramUsedGb}/${vramTotalGb}`
                              : "—"}
                          </span>
                        </td>
                        <td className="px-5 py-4 font-mono">
                          <span className="text-white font-semibold">
                            {cpuPct != null ? `${fmt(cpuPct)}%` : "—"}
                          </span>
                        </td>
                      </>
                    )}
                  </tr>
                ))}
                {orchestrators.length === 0 && (
                  <tr>
                    <td
                      colSpan={7}
                      className="px-5 py-8 text-center text-zinc-400"
                    >
                      Nenhum orquestrador registrado.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
