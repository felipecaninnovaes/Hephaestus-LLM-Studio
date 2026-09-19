"use client";

import { useCallback, useEffect, useState, useTransition } from "react";
import {
  IconDatabase,
  IconGrid,
  IconHardDrive,
  IconImage,
  IconLayers,
  IconList,
  IconRefresh,
  IconServer,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { getTelemetry, listJobs } from "@/lib/jobs";
import {
  listOrchestrators,
  listModels,
  getStorageUsage,
  STATUS_LABELS,
  STATUS_CLASSES,
  nodeMetrics,
  type Orchestrator,
  type ModelWeight,
  type StorageUsage,
} from "@/lib/monitoring";
import { Button, SegmentedControl, StatCard, GlassCard } from "@/components/ui";
import { OrchestratorCard } from "@/components/composite/OrchestratorCard";
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
  const [orchsUnavailable, setOrchsUnavailable] = useState(false);
  const [modelsUnavailable, setModelsUnavailable] = useState(false);
  const [isRefreshing, startTransition] = useTransition();

  const fetchDashboardData = useCallback(async () => {
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
          listOrchestrators().catch((err) => {
            if (err instanceof ApiError && err.code === "queue_unavailable") {
              setOrchsUnavailable(true);
            }
            return { items: [] };
          }),
          listModels().catch((err) => {
            if (err instanceof ApiError && err.code === "queue_unavailable") {
              setModelsUnavailable(true);
            }
            return { items: [] };
          }),
          getStorageUsage().catch(() => null),
        ]);

      if (telemData.status === "fulfilled") setTelemetry(telemData.value);
      if (datasetsRes.status === "fulfilled" && Array.isArray(datasetsRes.value))
        setDatasets(datasetsRes.value);
      if (jobsRes.status === "fulfilled" && jobsRes.value?.items)
        setJobs(jobsRes.value.items);
      if (orchData.status === "fulfilled") {
        setOrchestrators(orchData.value.items);
        setOrchsUnavailable(false);
      }
      if (modelsData.status === "fulfilled") {
        setModels(modelsData.value.items);
        setModelsUnavailable(false);
      }
      if (storageData.status === "fulfilled") setStorageUsage(storageData.value);
    } catch {
      // Mantém dados em caso de flutuação
    }
  }, []);

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
  }, [fetchDashboardData]);

  const handleManualRefresh = () => {
    startTransition(async () => {
      await fetchDashboardData();
    });
  };

  /* ── Derived metrics (honest — no invented numbers) ──────── */

  const totalImages = datasets.reduce((acc, d) => acc + (d.imagesCount || 0), 0);
  const totalLabeled = datasets.reduce((acc, d) => acc + (d.labeledCount || 0), 0);
  const labeledPct =
    totalImages > 0 ? Math.round((totalLabeled / totalImages) * 100) : 100;

  const activeJobsCount = telemetry?.jobsActive ?? jobs.filter((j) => j.status !== "done" && j.status !== "failed" && j.status !== "cancelled").length;
  const completedJobsCount = jobs.filter((j) => j.status === "done").length;

  const fmt = (v: number | null | undefined, decimals = 1, suffix = ""): string =>
    v != null ? `${v.toFixed(decimals)}${suffix}` : "—";

  return (
    <div className="min-h-full space-y-6 p-4 sm:p-6 lg:p-8">
      {/* Top Header */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
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
          iconColor="text-status-success"
        />
        <StatCard
          label="Modelos & Pesos"
          value={models.length}
          subtext={
            modelsUnavailable
              ? "Indisponível (manager fora)"
              : models.length > 0
                ? "pesos de treinos"
                : "Sem pesos gerados"
          }
          icon={<IconImage className="size-4" />}
          iconColor="text-status-telemetry"
        />
        <StatCard
          label="Storage Canônico"
          value={storageUsage ? formatBytes(storageUsage.totalBytes) : "—"}
          subtext="rastreados pelo banco · Bucket S3"
          icon={<IconHardDrive className="size-4" />}
          iconColor="text-status-alert"
        />
      </div>

      {/* Node View: Grid or List — gauges por nó (regra length===1 morreu — H.5 D7) */}
      {viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-5 xl:grid-cols-2">
          {orchestrators.map((node) => (
            <OrchestratorCard key={node.id} node={node} />
          ))}

          {/* Sem nó registrado */}
          {orchestrators.length === 0 && (
            <GlassCard className="p-5 text-center text-sm">
              {orchsUnavailable ? (
                <span className="text-status-alert">Indisponível (manager fora)</span>
              ) : (
                <span className="text-zinc-400">Nenhum orquestrador registrado no sistema.</span>
              )}
            </GlassCard>
          )}
        </div>
      ) : (
        /* Modo Tabela / Lista — sempre com colunas de telemetria por nó */
        <div className="glass-card overflow-hidden rounded-2xl shadow-2xl">
          <div className="overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-white/10 bg-white/[0.03] font-mono text-2xs text-zinc-400 uppercase tracking-[0.08em]">
                <tr>
                  <th className="px-5 py-3.5">Orquestrador</th>
                  <th className="px-5 py-3.5">Status</th>
                  <th className="px-5 py-3.5">Último Heartbeat</th>
                  <th className="px-5 py-3.5">Endpoint</th>
                  <th className="px-5 py-3.5">GPU</th>
                  <th className="px-5 py-3.5">VRAM</th>
                  <th className="px-5 py-3.5">CPU</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5">
                {orchestrators.map((orch) => {
                  const m = nodeMetrics(orch);
                  return (
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
                            <div className="font-mono text-2xs text-zinc-400">
                              {orch.endpoint}
                            </div>
                          </div>
                        </div>
                      </td>
                      <td className="px-5 py-4">
                        <span className={`inline-flex items-center rounded-lg border backdrop-blur-sm px-2 py-0.5 font-medium text-2xs font-mono ${
                          STATUS_CLASSES[orch.status] ?? STATUS_CLASSES.unknown
                        }`}>
                          {STATUS_LABELS[orch.status] ?? orch.status}
                        </span>
                      </td>
                      <td className="px-5 py-4 font-mono text-zinc-300">
                        {orch.lastHeartbeat ? formatRelativeTime(orch.lastHeartbeat) : "—"}
                      </td>
                      <td className="px-5 py-4 text-zinc-300 font-mono text-2xs">
                        <TruncatedText text={orch.endpoint} as="span" />
                      </td>
                      <td className="px-5 py-4 font-mono">
                        <span className="text-white font-semibold">
                          {m.gpuLabel ?? "—"}
                        </span>
                      </td>
                      <td className="px-5 py-4 font-mono">
                        <span className="text-white font-semibold">
                          {m.vramUsedGb && m.vramTotalGb
                            ? `${m.vramUsedGb}/${m.vramTotalGb} · ${fmt(m.vramPct, 1, "%")}`
                            : "—"}
                        </span>
                      </td>
                      <td className="px-5 py-4 font-mono">
                        <span className="text-white font-semibold">
                          {fmt(m.cpuPct, 1, "%")}
                        </span>
                      </td>
                    </tr>
                  );
                })}
                {orchestrators.length === 0 && (
                  <tr>
                    <td
                      colSpan={7}
                      className="px-5 py-8 text-center text-sm"
                    >
                      {orchsUnavailable ? (
                        <span className="text-status-alert">Indisponível (manager fora)</span>
                      ) : (
                        <span className="text-zinc-400">Nenhum orquestrador registrado.</span>
                      )}
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
