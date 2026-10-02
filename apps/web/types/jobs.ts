export type JobStatus =
  | "preparing"
  | "queued"
  | "dispatched"
  | "running"
  | "cancelling"
  | "done"
  | "failed"
  | "cancelled";

export type JobKind =
  | "yolo_train"
  | "autotracker"
  | "yolo_predict"
  | "autolabel"
  | "diffusion"
  | "diffusion_train"
  | "diffusion_generate";

export interface SubmitJobResponse {
  jobId: string;
  status: JobStatus;
  queuePosition: number | null;
}

export function friendlyJobError(
  error: string | null | undefined,
): string | null {
  if (!error) return null;
  const m = /^prepare_failed:([^:]*):([\s\S]*)$/.exec(error);
  if (m) {
    const code = m[1].trim() || "prep";
    const msg = m[2].trim() || "falha na preparação do pacote.";
    return `Falha ao preparar o pacote (${code}): ${msg}`;
  }
  return error;
}

export interface JobMetrics {
  epoch: number;
  boxLoss?: number;
  clsLoss?: number;
  dflLoss?: number;
  map50?: number;
  map5095?: number;
  loss?: number;
  lr?: number;
  step?: number;
  progress?: number;
  phase?: string;
  message?: string;
}

export interface JobMetricsResponse {
  items: JobMetrics[];
}

export interface MetricPointWithKey {
  seq: number;
  epoch: number | null;
  step: number;
  key: string;
  value: number;
  ts: string;
}

export interface MetricPointsResponse {
  items: MetricPointWithKey[];
  maxSeq: number;
  downsampled: boolean;
}

export interface JobArtifact {
  id: string;
  kind: string;
  path: string;
  md5: string;
  bytes: number;
}

export interface JobArtifactsResponse {
  items: JobArtifact[];
}

/* ── Alertas (fatia 3c): regras nan_detected / telemetry_stale ── */

export type JobAlertRuleId =
  | "nan_detected"
  | "vram_high"
  | "disk_high"
  | "telemetry_stale";

export type JobAlertSeverity = "warning" | "critical";

export interface JobAlert {
  id: string;
  jobId: string;
  ruleId: JobAlertRuleId;
  severity: JobAlertSeverity;
  message: string;
  firedAt: string;
  resolvedAt?: string | null;
}

export interface JobAlertsResponse {
  items: JobAlert[];
}

export type JobTerminalStatus = "done" | "failed" | "cancelled";

export interface JobDeletedResponse {
  id: string;
  status: JobTerminalStatus;
  artifacts: string[];
  objectKeys: string[];
  modelsDeleted: number;
  generationsPreserved: number;
}

export interface JobCleanupRequest {
  olderThanDays?: number | null;
  statuses?: JobTerminalStatus[] | null;
}

export interface JobCleanupResponse {
  deleted: number;
  jobs: JobDeletedResponse[];
  objectKeys: string[];
}

export interface JobTelemetryEvent {
  timestamp: string;
  phase: string;
  phaseMessage?: string | null;
  progress: number;
  step?: number | null;
  totalSteps?: number | null;
  epoch?: number | null;
  totalEpochs?: number | null;
  vramUsedGb?: number | null;
  vramReservedGb?: number | null;
  stepTimeSeconds?: number | null;
  speed?: string | null;
  etaSeconds?: number | null;
  etaFormatted?: string | null;
  metrics?: Record<string, number | string | boolean | null> | null;
}

/* ── Tipagem estrita de parâmetros de execução (eliminando casts 'as any') ── */

export interface DiffusionJobParams {
  baseModel?: string;
  triggerWord?: string;
  epochs?: number;
  batchSize?: number;
  learningRate?: string | number;
  rank?: number;
  alpha?: number;
  optimizer?: string;
  resolution?: number;
  mixedPrecision?: string;
  quantization?: string;
  gradientAccumulationSteps?: number;
  customModelId?: string | null;
  textEncoderModelId?: string | null;
  lrScheduler?: string;
  lrWarmupSteps?: number;
  enableBucket?: boolean;
  checkpointInterval?: number;
  epochOffset?: number;
  samplePrompt?: string | null;
  sampleInterval?: number;
  [key: string]: unknown;
}

export interface YoloJobParams {
  model?: string;
  batch?: number;
  imgsz?: number;
  optimizer?: string;
  epochs?: number;
  lr0?: number;
  lrf?: number;
  augment?: Record<string, boolean>;
  [key: string]: unknown;
}

export type JobParams =
  | DiffusionJobParams
  | YoloJobParams
  | Record<string, unknown>;

export interface Job {
  id: string;
  kind: JobKind;
  engine: string;
  model: string;
  mode: string | null;
  datasetId: string | null;
  status: JobStatus;
  queueReason: string | null;
  queuePosition: number | null;
  progress: number;
  epoch: number | null;
  step: number | null;
  totalSteps?: number | null;
  totalEpochs?: number | null;
  metrics: Record<string, unknown> | null;
  vramMinGb: number | null;
  orchestratorId: string | null;
  orchestratorName?: string | null;
  orchestratorKind?: "docker" | "slurm" | "local" | "remoto" | null;
  orchestratorFallback: boolean;
  createdAt: string;
  finishedAt: string | null;
  error?: string | null;
  phase?: string | null;
  phaseMessage?: string | null;
  vramUsedGb?: number | null;
  vramReservedGb?: number | null;
  params?: JobParams | null;
}

export interface JobListResponse {
  items: Job[];
  total: number;
}

export type JobErrorCode =
  | "invalid_request"
  | "dataset_not_ready"
  | "queue_unavailable"
  | "not_found"
  | "job_not_done"
  | "storage_unavailable";

export function jobErrorMessage(code: string): string {
  switch (code) {
    case "invalid_request":
      return "Parâmetros de treino inválidos.";
    case "dataset_not_ready":
      return "O dataset não está pronto para treino.";
    case "queue_unavailable":
      return "Fila de treino indisponível — tente novamente.";
    case "not_found":
      return "Job não encontrado.";
    default:
      return "Falha ao criar job de treino.";
  }
}

/* ── Linhagem dataset→job→checkpoint→resume→geração (fatia 5b) ── */

export type LineageNodeKind = "dataset" | "job" | "checkpoint" | "generation";

export interface LineageNode {
  id: string;
  kind: LineageNodeKind;
  label: string;
  status?: string | null;
  createdAt?: string | null;
  epoch?: number | null;
}

export type LineageEdgeKind = "trains" | "produced" | "resumed_by" | "used_by";

export interface LineageEdge {
  from: string;
  to: string;
  kind: LineageEdgeKind;
}

export interface LineageGraph {
  nodes: LineageNode[];
  edges: LineageEdge[];
}
