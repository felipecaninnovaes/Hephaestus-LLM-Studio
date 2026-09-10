import { apiFetch } from "@/lib/api";

/* ── Shared Status Helpers ─────────────────────────────────── */

export const STATUS_LABELS: Record<string, string> = {
  online: "Online",
  degraded: "Degradado",
  offline: "Offline",
  revoked: "Revogado",
  unknown: "Desconhecido",
};

export const STATUS_CLASSES: Record<string, string> = {
  online: "text-[#34d399] bg-[#34d399]/10 border-[#34d399]/30",
  degraded: "text-[#f59e0b] bg-[#f59e0b]/10 border-[#f59e0b]/30",
  offline: "text-zinc-400 bg-zinc-800/40 border-zinc-700/50",
  revoked: "text-zinc-500 bg-zinc-800/40 border-zinc-700/50",
  unknown: "text-zinc-400 bg-zinc-800/40 border-zinc-700/50",
};

/** Deriva métricas de um único nó (fonte única — D7 H.5). */
export function nodeMetrics(orch: Orchestrator) {
  const hasGpu = (orch.gpus?.length ?? 0) > 0 && orch.vramTotal != null;
  const cpuPct =
    orch.measured && orch.cpu != null
      ? Math.min(100, Math.max(0, orch.cpu))
      : null;
  const ramUsedGb =
    orch.measured && orch.ram != null
      ? (orch.ram / (1024 * 1024 * 1024)).toFixed(1)
      : null;
  const ramTotalGb =
    orch.measured && orch.ramTotal != null
      ? (orch.ramTotal / (1024 * 1024 * 1024)).toFixed(1)
      : null;
  const vramUsedGb =
    orch.measured && orch.vramUsed != null
      ? (orch.vramUsed / 1024).toFixed(1)
      : null;
  const vramTotalGb =
    orch.measured && orch.vramTotal != null
      ? (orch.vramTotal / 1024).toFixed(1)
      : null;
  const vramPct =
    orch.measured &&
    orch.vramUsed != null &&
    orch.vramTotal != null &&
    orch.vramTotal > 0
      ? Math.min(100, Math.max(0, (orch.vramUsed / orch.vramTotal) * 100))
      : null;
  const gpuLabel = hasGpu ? orch.gpus[0] : null;
  return {
    cpuPct,
    ramUsedGb,
    ramTotalGb,
    vramUsedGb,
    vramTotalGb,
    vramPct,
    gpuLabel,
    hasGpu,
  };
}

/* ── Types ──────────────────────────────────────────────────── */

export interface Orchestrator {
  id: string;
  name: string;
  kind: "local" | "remoto";
  endpoint: string;
  status: string;
  lastHeartbeat: string | null;
  /* Telemetria por nó (enriquecida — D2/D7 H.5) */
  measured: boolean;
  cpu: number | null;
  ram: number | null;
  ramTotal: number | null;
  vramUsed: number | null;
  vramTotal: number | null;
  vramTotalGb: number | null;
  gpus: string[];
  jobsActive: number;
}

export interface OrchestratorsResponse {
  items: Orchestrator[];
}

export interface ModelWeight {
  id: string;
  name: string;
  engine: string;
  model: string;
  jobId: string;
  bytes: number;
  createdAt: string;
}

export interface ModelsResponse {
  items: ModelWeight[];
}

export interface StorageUsage {
  datasetsBytes: number;
  artifactsBytes: number;
  totalBytes: number;
  measured: boolean;
}

export interface HealthResponse {
  status: string;
  service: string;
  auth: string;
  version: string;
}

/* ── API Wrappers ────────────────────────────────────────────── */

/** GET /api/environments — lista real da tabela do manager (alias de /api/orchestrators). */
export function listOrchestrators(): Promise<OrchestratorsResponse> {
  return apiFetch("/api/environments");
}

/** POST /api/environments/adopt — adota ou re-adota um orquestrador. */
export function adoptOrchestrator(body: {
  name: string;
  endpoint: string;
  kind: "local" | "remoto";
  pairingCode: string;
}): Promise<Orchestrator> {
  return apiFetch("/api/environments/adopt", {
    method: "POST",
    body,
  });
}

/** POST /api/environments/:id/revoke — revoga um orquestrador (status → revoked). */
export function revokeOrchestrator(id: string): Promise<void> {
  return apiFetch(`/api/environments/${id}/revoke`, {
    method: "POST",
  });
}

/** Mapeia erros de adopt/revoke para mensagens estáticas legíveis. */
export function orchestratorErrorMessage(status: number, code: string): string {
  if (code === "pairing_invalid") {
    return "Código de pareamento inválido ou orquestrador inalcançável.";
  }
  if (status === 503 || code === "queue_unavailable") {
    return "Indisponível (manager fora).";
  }
  if (code === "not_found") {
    return "Orquestrador não encontrado.";
  }
  return "Falha na operação de orquestrador.";
}

/** GET /api/models — pesos derivados de job_artifacts.kind='model'. */
export function listModels(): Promise<ModelsResponse> {
  return apiFetch("/api/models");
}

/** GET /api/storage/usage — soma SQL por dono (datasets + artifacts). */
export function getStorageUsage(): Promise<StorageUsage> {
  return apiFetch("/api/storage/usage");
}

/** GET /health — versão de produto e status do serviço. */
export function getHealth(): Promise<HealthResponse> {
  return fetch("/health", { credentials: "same-origin" }).then((r) => {
    if (!r.ok) throw new Error(`Health check failed: ${r.status}`);
    return r.json() as Promise<HealthResponse>;
  });
}
