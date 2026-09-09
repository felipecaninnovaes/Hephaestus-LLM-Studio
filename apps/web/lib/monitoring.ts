import { apiFetch } from "@/lib/api";

/* ── Types ──────────────────────────────────────────────────── */

export interface Orchestrator {
  id: string;
  name: string;
  kind: "local" | "remoto";
  endpoint: string;
  status: string;
  lastHeartbeat: string | null;
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

/** GET /api/orchestrators — lista real da tabela do manager. */
export function listOrchestrators(): Promise<OrchestratorsResponse> {
  return apiFetch("/api/orchestrators");
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
