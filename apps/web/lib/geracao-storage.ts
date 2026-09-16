"use client";

/* ═══════════════════════════════════════════════════════════════════
   geracao-storage — persistência local do form de geração (Slice F1/003)
   Key versionada `geracao:form:v1`, formato { version: 1, state }.
   Persiste SOMENTE config do form (modelo, LoRAs, prompt, resolução,
   steps, CFG, seed, batch, quantização, destilada, flags de UI do form).
   NUNCA: resultados, histórico, jobs, segredos, orchestrator selecionado
   (disponibilidade de nós muda entre sessões) ou estado de execução.
   ═══════════════════════════════════════════════════════════════════ */

import type { LoraRef } from "@/types/studio";

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
