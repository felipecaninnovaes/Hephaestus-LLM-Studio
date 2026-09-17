"use client";

/* ═══════════════════════════════════════════════════════════════════
   geracao-storage — persistência local do form de geração (Slice F1/003)
   Key versionada `geracao:form:v1`, formato { version: 1, state }.
   Persiste SOMENTE config do form (modelo, LoRAs, prompt, resolução,
   steps, CFG, seed, batch, quantização, destilada, flags de UI do form).
   NUNCA: resultados, histórico, jobs, segredos, orchestrator selecionado
   (disponibilidade de nós muda entre sessões) ou estado de execução.
   ═══════════════════════════════════════════════════════════════════ */

import type { Generation, LoraRef } from "@/types/studio";

export const GERACAO_FORM_KEY = "geracao:form:v1";
const GERACAO_FORM_VERSION = 1;

export type GeracaoModelMode = "preset" | "custom";
export type GeracaoBaseModel = "flux-2-klein-4b" | "sdxl" | "sd15";
export type GeracaoQuantization = "4bit" | "8bit" | "none";

export interface GeracaoFormState {
  modelMode: GeracaoModelMode;
  baseModel: GeracaoBaseModel;
  customModelId: string;
  distilled: boolean;
  loras: LoraRef[];
  prompt: string;
  negativePrompt: string;
  showNegative: boolean;
  width: number;
  height: number;
  steps: number;
  guidanceScale: number;
  seed: number;
  isLockedSeed: boolean;
  quantization: GeracaoQuantization;
  batchSize: number;
}

const BASE_MODELS: readonly GeracaoBaseModel[] = ["flux-2-klein-4b", "sdxl", "sd15"];
const QUANTIZATIONS: readonly GeracaoQuantization[] = ["4bit", "8bit", "none"];

export function randomGeracaoSeed(): number {
  return Math.floor(Math.random() * 10_000_000);
}

/* Defaults espelham os useState iniciais de GenerationPanel. Seed é
   rolada a cada chamada (igual ao initializer do componente). */
export function createDefaultGeracaoForm(): GeracaoFormState {
  return {
    modelMode: "preset",
    baseModel: "flux-2-klein-4b",
    customModelId: "",
    distilled: true,
    loras: [],
    prompt: "",
    negativePrompt: "",
    showNegative: false,
    width: 1024,
    height: 1024,
    steps: 4,
    guidanceScale: 1.0,
    seed: randomGeracaoSeed(),
    isLockedSeed: false,
    quantization: "4bit",
    batchSize: 1,
  };
}

/* ── Validação / clamp (limites iguais aos do submit e dos Sliders) ── */

function clampInt(v: unknown, min: number, max: number, fallback: number): number {
  if (typeof v !== "number" || !Number.isFinite(v)) return fallback;
  return Math.min(max, Math.max(min, Math.round(v)));
}

function clampFloat(v: unknown, min: number, max: number, fallback: number): number {
  if (typeof v !== "number" || !Number.isFinite(v)) return fallback;
  return Math.min(max, Math.max(min, v));
}

function asString(v: unknown, maxLen: number): string | null {
  if (typeof v !== "string") return null;
  return v.slice(0, maxLen);
}

function sanitizeLoras(v: unknown): LoraRef[] | null {
  if (!Array.isArray(v)) return null;
  const out: LoraRef[] = [];
  for (const item of v) {
    if (typeof item !== "object" || item === null) continue;
    const rec = item as Record<string, unknown>;
    if (typeof rec.modelId !== "string" || rec.modelId.length === 0) continue;
    const scale = typeof rec.scale === "number" && Number.isFinite(rec.scale)
      ? Math.min(2, Math.max(0, rec.scale))
      : 1;
    out.push({ modelId: rec.modelId.slice(0, 256), scale });
    if (out.length >= 10) break;
  }
  return out;
}

/* Primeiro valor presente entre aliases (params mistura snake_case do
   manager com camelCase do wire — ex.: `base_model`/`baseModel`,
   `guidance_scale`/`guidanceScale`, `batchSize`/`batch_size`). */
function firstParam(params: Record<string, unknown>, keys: readonly string[]): unknown {
  for (const k of keys) {
    const v = params[k];
    if (v !== undefined && v !== null) return v;
  }
  return undefined;
}

/* Número defensivo (aceita string numérica de linhas antigas). */
function numOr(v: unknown, fallback: number): number {
  if (typeof v === "number" && Number.isFinite(v)) return v;
  if (typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v))) {
    return Number(v);
  }
  return fallback;
}

export type PartialGeracaoForm = Partial<GeracaoFormState>;

/* Hidrata do localStorage com validação campo a campo. Retorna null se
   ausente/corrompido; retorna parcial (só campos válidos) caso contrário.
   customModelId é mantido mesmo fora da lista atual de checkpoints — a
   UI trata graciosamente (placeholder "Faça upload em Modelos & Pesos"). */
export function loadGeracaoForm(): PartialGeracaoForm | null {
  try {
    const raw = window.localStorage.getItem(GERACAO_FORM_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const envelope = parsed as Record<string, unknown>;
    if (envelope.version !== GERACAO_FORM_VERSION) return null;
    if (typeof envelope.state !== "object" || envelope.state === null) return null;
    const s = envelope.state as Record<string, unknown>;
    const defaults = createDefaultGeracaoForm();
    const out: PartialGeracaoForm = {};

    if (s.modelMode === "preset" || s.modelMode === "custom") out.modelMode = s.modelMode;
    if (typeof s.baseModel === "string" && (BASE_MODELS as readonly string[]).includes(s.baseModel)) {
      out.baseModel = s.baseModel as GeracaoBaseModel;
    }
    const customId = asString(s.customModelId, 256);
    if (customId !== null) out.customModelId = customId;
    if (typeof s.distilled === "boolean") out.distilled = s.distilled;
    const loras = sanitizeLoras(s.loras);
    if (loras !== null) out.loras = loras;
    const prompt = asString(s.prompt, 4000);
    if (prompt !== null) out.prompt = prompt;
    const negative = asString(s.negativePrompt, 4000);
    if (negative !== null) out.negativePrompt = negative;
    if (typeof s.showNegative === "boolean") out.showNegative = s.showNegative;
    if (s.width !== undefined) out.width = clampInt(s.width, 256, 2048, defaults.width);
    if (s.height !== undefined) out.height = clampInt(s.height, 256, 2048, defaults.height);
    if (s.steps !== undefined) out.steps = clampInt(s.steps, 1, 50, defaults.steps);
    if (s.guidanceScale !== undefined) {
      out.guidanceScale = clampFloat(s.guidanceScale, 1, 15, defaults.guidanceScale);
    }
    if (s.seed !== undefined) out.seed = clampInt(s.seed, 0, 99_999_999, defaults.seed);
    if (typeof s.isLockedSeed === "boolean") out.isLockedSeed = s.isLockedSeed;
    if (typeof s.quantization === "string" && (QUANTIZATIONS as readonly string[]).includes(s.quantization)) {
      out.quantization = s.quantization as GeracaoQuantization;
    }
    if (s.batchSize !== undefined) out.batchSize = clampInt(s.batchSize, 1, 8, defaults.batchSize);

    return out;
  } catch {
    return null;
  }
}

export function saveGeracaoForm(state: GeracaoFormState): void {
  try {
    window.localStorage.setItem(
      GERACAO_FORM_KEY,
      JSON.stringify({ version: GERACAO_FORM_VERSION, state }),
    );
  } catch {
    /* storage indisponível/cheio (modo privado, quota) — form segue vivo */
  }
}

export function clearGeracaoForm(): void {
  try {
    window.localStorage.removeItem(GERACAO_FORM_KEY);
  } catch {
    /* noop */
  }
}

/* ── Marker de conclusão (Slice F2/002) ──
   Canal cross-tab Gerar → Galeria: o caso de uso real são DUAS ABAS do
   navegador (uma em Gerar, outra em Galeria). `window.CustomEvent` não
   cruza abas; o evento `storage` do localStorage cruza — por isso ele é
   o canal primário. O Panel grava; a Gallery escuta `storage` + refetch
   em `visibilitychange`/`focus` (cobre mesma-aba, onde `storage` não
   dispara na aba de origem). Valor JSON { completedAt, jobId }. */

export const GERACAO_COMPLETED_KEY = "geracao:lastCompletedAt";

export interface GeracaoCompletedMarker {
  completedAt: string;
  jobId: string;
}

export function notifyGeracaoCompleted(jobId: string): void {
  try {
    const marker: GeracaoCompletedMarker = {
      completedAt: new Date().toISOString(),
      jobId,
    };
    window.localStorage.setItem(GERACAO_COMPLETED_KEY, JSON.stringify(marker));
  } catch {
    /* storage indisponível/cheio — galeria cobre via focus/visibility */
  }
}

export function readGeracaoCompletedMarker(): GeracaoCompletedMarker | null {
  try {
    const raw = window.localStorage.getItem(GERACAO_COMPLETED_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const rec = parsed as Record<string, unknown>;
    if (typeof rec.completedAt !== "string" || typeof rec.jobId !== "string") {
      return null;
    }
    return { completedAt: rec.completedAt, jobId: rec.jobId };
  } catch {
    return null;
  }
}

/* ── Canal Galeria → Gerador (Slice F4/007) ──
   Evento canônico mesma-aba: `hephaestus:apply-geracao-form`. O Panel
   escuta e re-hidrata do storage; se desmontado, a hidratação no mount
   já pega o storage (layout atual monta UMA aba por vez — Galeria grava
   → switch-tab p/ "gerar" → Panel hidrata no mount).
   Cross-tab (Gerar aberta em OUTRA aba do navegador, caso F2): o write
   do storage atualiza na remontagem, SEM live-update cross-tab por aqui
   (o Panel não escuta `storage` p/ o form; sem polling permanente — mesma
   decisão do F2). Limitação documentada, não bug. */

export const GERACAO_APPLY_FORM_EVENT = "hephaestus:apply-geracao-form";

/* Snapshot fiel p/ reprodução (clipboard): params vencem, colunas
   top-level (width/height/seed/prompt) cobrem linhas antigas/incompletas.
   Sem clamp aqui — fidelidade; o clamp vive em geracaoFormFromGeneration.
   Mapeamento honesto (F4 fixes): o manager persiste `params.loras` como
   [{s3_key,md5,scale}] e a engine emite [{path,scale}] — sem UUID. Itens
   sem `modelId` NÃO são reaplicáveis: vão p/ `lorasRaw` + `hasLoraResidue`
   (transparência no JSON), e `loras` carrega só UUIDs reaplicáveis.
   Mesmo p/ custom: `params.custom_checkpoint`/`custom_model_path`+`arch`
   nunca viram UUID — só `customModelId`/`custom_model_id` legado ativa
   modelMode="custom"; o resto vira resíduo documental. Ausentes = null
   (nunca a string "unknown"). */
export interface GenerationReproSnapshot {
  baseModel: string | null;
  customModelId: string | null;
  customModelPath: string | null;
  customCheckpoint: unknown;
  hasCustomResidue: boolean;
  prompt: string;
  negativePrompt: string | null;
  width: number;
  height: number;
  steps: number;
  guidanceScale: number;
  seed: number;
  batchSize: number;
  quantization: string | null;
  distilled: boolean;
  loras: LoraRef[];
  lorasRaw: Record<string, unknown>[];
  hasLoraResidue: boolean;
  modelMode: GeracaoModelMode;
  seedLocked: boolean;
}

export const GERACAO_LORA_RESIDUE_WARNING =
  "A geração usava LoRA(s) que não podem ser reaplicados automaticamente; configs aplicadas sem LoRA";
export const GERACAO_CUSTOM_RESIDUE_WARNING =
  "Checkpoint custom não reaplicável automaticamente — selecione o modelo em Modelos & Pesos";

/* LoRAs do snapshot: aceita os 3 shapes (modelId UUID reaplicável,
   path da engine, s3_key do manager) mantendo `scale`. Só `modelId`
   válido entra em `loras` (form); o resto é resíduo documental. */
function parseSnapshotLoras(raw: unknown): {
  loras: LoraRef[];
  lorasRaw: Record<string, unknown>[];
  hasLoraResidue: boolean;
} {
  if (!Array.isArray(raw)) return { loras: [], lorasRaw: [], hasLoraResidue: false };
  const loras: LoraRef[] = [];
  const lorasRaw: Record<string, unknown>[] = [];
  for (const item of raw) {
    if (typeof item !== "object" || item === null) continue;
    const rec = item as Record<string, unknown>;
    const scale = typeof rec.scale === "number" && Number.isFinite(rec.scale)
      ? Math.min(2, Math.max(0, rec.scale))
      : 1;
    if (typeof rec.modelId === "string" && rec.modelId.length > 0) {
      if (loras.length < 10) {
        loras.push({ modelId: rec.modelId.slice(0, 256), scale });
      }
    } else {
      if (lorasRaw.length < 10) lorasRaw.push(rec);
    }
  }
  return { loras, lorasRaw, hasLoraResidue: lorasRaw.length > 0 };
}

function nonEmptyString(v: unknown, maxLen: number): string | null {
  const s = asString(v, maxLen);
  return s !== null && s.length > 0 ? s : null;
}

export function generationReproSnapshot(gen: Generation): GenerationReproSnapshot {
  const params: Record<string, unknown> = gen.params ?? {};
  const customRaw = nonEmptyString(firstParam(params, ["customModelId", "custom_model_id"]), 256);
  const customModelPath = nonEmptyString(firstParam(params, ["custom_model_path", "customModelPath"]), 1024);
  const customCheckpointRaw = firstParam(params, ["custom_checkpoint", "customCheckpoint"]);
  const customCheckpoint = customCheckpointRaw !== undefined && customCheckpointRaw !== null
    ? customCheckpointRaw
    : null;
  const arch = nonEmptyString(firstParam(params, ["arch"]), 64);
  const hasCustomResidue = customRaw === null
    && (customModelPath !== null || customCheckpoint !== null || arch !== null);
  const negTop = typeof gen.negativePrompt === "string" && gen.negativePrompt.length > 0
    ? gen.negativePrompt
    : null;
  const negParam = nonEmptyString(firstParam(params, ["negative_prompt", "negativePrompt"]), 4000);
  const distilledRaw = firstParam(params, ["distilled"]);
  const rawBase = nonEmptyString(firstParam(params, ["base_model", "baseModel"]), 64);
  const baseModel = (BASE_MODELS as readonly string[]).includes(rawBase ?? "")
    ? rawBase
    : ((arch !== null && (BASE_MODELS as readonly string[]).includes(arch)) ? arch : null);
  const quantRaw = nonEmptyString(firstParam(params, ["quantization"]), 16);
  const { loras, lorasRaw, hasLoraResidue } = parseSnapshotLoras(firstParam(params, ["loras"]));
  return {
    baseModel,
    customModelId: customRaw,
    customModelPath,
    customCheckpoint,
    hasCustomResidue,
    prompt: gen.prompt,
    negativePrompt: negTop ?? negParam,
    width: numOr(firstParam(params, ["width"]), gen.width),
    height: numOr(firstParam(params, ["height"]), gen.height),
    steps: numOr(firstParam(params, ["steps"]), 4),
    guidanceScale: numOr(firstParam(params, ["guidance_scale", "guidanceScale"]), 1),
    seed: numOr(firstParam(params, ["seed"]), gen.seed),
    batchSize: numOr(firstParam(params, ["batchSize", "batch_size"]), 1),
    quantization: quantRaw,
    distilled: typeof distilledRaw === "boolean" ? distilledRaw : false,
    loras,
    lorasRaw,
    hasLoraResidue,
    modelMode: customRaw !== null ? "custom" : "preset",
    seedLocked: true,
  };
}

/* JSON legível p/ clipboard ("Copiar configs"). */
export function generationConfigsJson(gen: Generation): string {
  return JSON.stringify(generationReproSnapshot(gen), null, 2);
}

/* Constrói GeracaoFormState a partir de uma geração, com o MESMO
   clamp/validação da hidratação (loadGeracaoForm). Seed travada p/
   reproduzir exatamente (usuário pode destravar no panel).
   customModelId fora da lista atual de checkpoints: mantido com
   modelMode="custom" — a validação/graciosidade existente do panel
   lida (placeholder "Faça upload em Modelos & Pesos"). */
export interface GeracaoFormFromGenerationResult {
  form: GeracaoFormState;
  hasLoraResidue: boolean;
  hasCustomResidue: boolean;
  warnings: string[];
}

export function geracaoFormFromGeneration(gen: Generation): GeracaoFormFromGenerationResult {
  const defaults = createDefaultGeracaoForm();
  const params: Record<string, unknown> = gen.params ?? {};
  const snap = generationReproSnapshot(gen);
  const customModelId = snap.customModelId ?? "";
  const negativePrompt = snap.negativePrompt ?? "";
  const quantized = firstParam(params, ["quantization"]);
  const distilledRaw = firstParam(params, ["distilled"]);
  const warnings: string[] = [];
  if (snap.hasLoraResidue) warnings.push(GERACAO_LORA_RESIDUE_WARNING);
  if (snap.hasCustomResidue) warnings.push(GERACAO_CUSTOM_RESIDUE_WARNING);
  const form: GeracaoFormState = {
    modelMode: customModelId.length > 0 ? "custom" : "preset",
    baseModel: snap.baseModel !== null
      && (BASE_MODELS as readonly string[]).includes(snap.baseModel)
      ? (snap.baseModel as GeracaoBaseModel)
      : defaults.baseModel,
    customModelId,
    distilled: typeof distilledRaw === "boolean" ? distilledRaw : defaults.distilled,
    loras: snap.loras,
    prompt: snap.prompt.slice(0, 4000),
    negativePrompt: negativePrompt.slice(0, 4000),
    showNegative: negativePrompt.length > 0,
    width: clampInt(snap.width, 256, 2048, defaults.width),
    height: clampInt(snap.height, 256, 2048, defaults.height),
    steps: clampInt(snap.steps, 1, 50, defaults.steps),
    guidanceScale: clampFloat(snap.guidanceScale, 1, 15, defaults.guidanceScale),
    seed: clampInt(snap.seed, 0, 99_999_999, defaults.seed),
    isLockedSeed: true,
    quantization: typeof quantized === "string"
      && (QUANTIZATIONS as readonly string[]).includes(quantized)
      ? (quantized as GeracaoQuantization)
      : defaults.quantization,
    batchSize: clampInt(snap.batchSize, 1, 8, defaults.batchSize),
  };
  return {
    form,
    hasLoraResidue: snap.hasLoraResidue,
    hasCustomResidue: snap.hasCustomResidue,
    warnings,
  };
}

/* Grava o form + despacha o evento canônico (mesma-aba). */
export function publishGeracaoForm(state: GeracaoFormState): void {
  saveGeracaoForm(state);
  window.dispatchEvent(new CustomEvent(GERACAO_APPLY_FORM_EVENT));
}
