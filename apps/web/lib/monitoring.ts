import { apiFetch } from "@/lib/api";

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
