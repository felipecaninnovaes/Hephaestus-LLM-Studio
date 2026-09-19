import type { SelectOption } from "@/components/ui/Select";
import type {
  GeracaoUpscaleModel,
} from "@/lib/geracao-storage";
import type { LoraRef } from "@/types/studio";

export interface GeneratedImageItem {
  jobId: string;
  imageUrl: string;
  thumbUrl?: string | null;
  prompt: string;
  negativePrompt?: string;
  baseModel: string;
  customModelId?: string;
  seed: number;
  steps: number;
  guidanceScale: number;
  quantization: string;
  sampler?: string;
  upscale?: { model: GeracaoUpscaleModel; scale: 2 | 4 } | null;
  distilled?: boolean;
  loras: LoraRef[];
  width: number;
  height: number;
  batchIndex?: number;
  createdAt: string;
}

export const BASE_MODEL_OPTIONS: SelectOption<string>[] = [
  {
    value: "flux-2-klein-4b",
    label: "FLUX.2 Klein 4B",
    description: "4B Params · Flow Matching · Ultrarrápido",
  },
  {
    value: "sdxl",
    label: "SDXL 1.0",
    description: "Dual CLIP · Resolução Nativa 1024x1024",
  },
  {
    value: "sd15",
    label: "Stable Diffusion 1.5",
    description: "Arquitetura Clássica Leve · 512x512",
  },
];

export const SQUARE_RESOLUTION_OPTIONS = [
  256, 512, 768, 1024, 1280, 1328, 1536, 2048,
];

export const ASPECT_RESOLUTION_PRESETS: {
  label: string;
  width: number;
  height: number;
}[] = [
  { label: "16:9 · 1792×1008", width: 1792, height: 1008 },
  { label: "16:9 · 1280×720", width: 1280, height: 720 },
  { label: "9:16 · 1008×1792", width: 1008, height: 1792 },
  { label: "9:16 · 720×1280", width: 720, height: 1280 },
  { label: "4:3 · 1344×1008", width: 1344, height: 1008 },
  { label: "4:3 · 1024×768", width: 1024, height: 768 },
  { label: "3:2 · 1536×1024", width: 1536, height: 1024 },
  { label: "3:2 · 1152×768", width: 1152, height: 768 },
];

export const SAMPLER_LABELS: Record<string, string> = {
  default: "Padrão",
  euler: "Euler",
  euler_a: "Euler A",
  heun: "Heun",
  dpmpp_2m: "DPM++ 2M",
  dpmpp_2m_karras: "DPM++ 2M Karras",
  dpmpp_2m_sde: "DPM++ 2M SDE",
  dpmpp_2m_sde_karras: "DPM++ 2M SDE Karras",
  dpmpp_sde: "DPM SDE",
  ddim: "DDIM",
};

export const QUANTIZATION_OPTIONS: SelectOption<string>[] = [
  {
    value: "4bit",
    label: "4-bit NF4",
    description: "~8–10 GB VRAM",
  },
  {
    value: "6bit",
    label: "6-bit (TorchAo)",
    description: "~10 GB VRAM",
  },
  {
    value: "8bit",
    label: "8-bit BnB",
    description: "~12–14 GB VRAM",
  },
  {
    value: "2bit",
    label: "2-bit (TorchAo)",
    description: "~7 GB VRAM — degradação visível, p/ testes",
  },
  {
    value: "none",
    label: "FP16",
    description: "~18+ GB VRAM",
  },
];

export const UPSCALE_MODEL_DEFAULT: GeracaoUpscaleModel = "ultrasharp";

export const UPSCALE_MODEL_OPTIONS: SelectOption<GeracaoUpscaleModel>[] = [
  {
    value: "ultrasharp",
    label: "UltraSharp (retratos, preserva textura)",
  },
  {
    value: "4x",
    label: "x4plus (generalista; suaviza pele)",
  },
  {
    value: "siax",
    label: "NMKD-Siax (nitidez + textura)",
  },
];

export const UPSCALE_MODEL_SHORT_LABEL: Record<GeracaoUpscaleModel, string> = {
  ultrasharp: "ultrasharp",
  "4x": "x4plus",
  siax: "siax",
};

export const INIT_STRENGTH_DEFAULT = 0.6;
export const INIT_STRENGTH_MIN = 0.05;
export const INIT_STRENGTH_MAX = 0.95;

export function clampInitStrength(v: number): number {
  if (!Number.isFinite(v)) return INIT_STRENGTH_DEFAULT;
  const stepped = Math.round(v / 0.05) * 0.05;
  return Math.min(INIT_STRENGTH_MAX, Math.max(INIT_STRENGTH_MIN, stepped));
}
