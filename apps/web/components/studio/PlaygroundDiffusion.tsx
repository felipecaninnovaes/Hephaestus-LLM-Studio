"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  IconAlertTriangle,
  IconCheck,
  IconCopy,
  IconDice,
  IconDownload,
  IconImage,
  IconInfo,
  IconPlay,
  IconRefresh,
  IconSliders,
  IconSparkles,
  IconStop,
  IconZap,
  IconZoomIn,
} from "@/components/icons";
import { Button, EmptyState, GlassCard, Select, type SelectOption, showToast } from "@/components/ui";
import { listJobs, getJob } from "@/lib/jobs";
import { listModels } from "@/lib/models";
import { getGeneratedImageUrl, startDiffusionGenerateJob } from "@/lib/playground";
import type { Job, Model } from "@/types/studio";
import { diffusionGenerateErrorMessage } from "@/types/studio";
import { ApiError } from "@/lib/api";
import NodeSelect from "@/components/studio/NodeSelect";

export interface GeneratedImageItem {
  jobId: string;
  imageUrl: string;
  prompt: string;
  negativePrompt?: string;
  baseModel: string;
  seed: number;
  steps: number;
  guidanceScale: number;
  quantization: string;
  loraName?: string;
  loraScale?: number;
  width: number;
  height: number;
  createdAt: string;
}

const BASE_MODEL_OPTIONS: SelectOption<string>[] = [
  {
    value: "flux-2-klein-4b",
    label: "FLUX.2 Klein 4B",
    description: "4B Params · Flow Matching · Single Qwen3 Encoder · Ultrarrápido",
  },
  {
    value: "sdxl",
    label: "SDXL 1.0 (Stability AI)",
    description: "Dual CLIP Text Encoders · Resolução Nativa 1024x1024",
  },
  {
    value: "sd15",
    label: "Stable Diffusion 1.5",
    description: "Arquitetura Clássica Leve · Resolução 512x512",
  },
];

const ASPECT_RATIO_PRESETS = [
  { label: "1:1 Quadrado", width: 1024, height: 1024 },
  { label: "16:9 Paisagem", width: 1344, height: 768 },
  { label: "9:16 Retrato", width: 768, height: 1344 },
  { label: "4:3 Clássico", width: 1152, height: 864 },
  { label: "Eco 512", width: 512, height: 512 },
];

const QUANTIZATION_OPTIONS: SelectOption<string>[] = [
  {
    value: "4bit",
    label: "4-bit NF4 (Recomendado)",
    description: "~8–10 GB VRAM · Ideal para RTX 3060 12GB",
  },
  {
    value: "8bit",
    label: "8-bit BitsAndBytes",
    description: "~12–14 GB VRAM · GPUs de 16GB+",
  },
  {
    value: "none",
    label: "FP16 Pleno (Sem quantização)",
    description: "~18–24 GB VRAM · Nós de alta capacidade",
  },
];

export default function PlaygroundDiffusion() {
  /* ── State: Models ── */
  const [models, setModels] = useState<Model[]>([]);
  const [loadingModels, setLoadingModels] = useState(true);

  /* ── Form Parameters ── */
  const [baseModel, setBaseModel] = useState<"flux-2-klein-4b" | "sdxl" | "sd15">("flux-2-klein-4b");
  const [selectedLoRAId, setSelectedLoRAId] = useState<string>("");
  const [loraScale, setLoraScale] = useState<number>(1.0);
  const [prompt, setPrompt] = useState<string>("");
  const [negativePrompt, setNegativePrompt] = useState<string>("");
  const [showNegative, setShowNegative] = useState<boolean>(false);
  const [width, setWidth] = useState<number>(1024);
  const [height, setHeight] = useState<number>(1024);
  const [steps, setSteps] = useState<number>(20);
  const [guidanceScale, setGuidanceScale] = useState<number>(3.5);
  const [seed, setSeed] = useState<number>(() => Math.floor(Math.random() * 1000000));
  const [isLockedSeed, setIsLockedSeed] = useState<boolean>(false);
  const [quantization, setQuantization] = useState<"4bit" | "8bit" | "none">("4bit");
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);

  /* ── Execution & Result State ── */
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [activeJobId, setActiveJobId] = useState<string | null>(null);
  const [activeJob, setActiveJob] = useState<Job | null>(null);
  const [currentDisplayItem, setCurrentDisplayItem] = useState<GeneratedImageItem | null>(null);
  const [history, setHistory] = useState<GeneratedImageItem[]>([]);
  const [copiedPrompt, setCopiedPrompt] = useState<boolean>(false);
  const [lightboxOpen, setLightboxOpen] = useState<boolean>(false);

  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const submittedParamsRef = useRef<{
    prompt: string;
    negativePrompt?: string;
    baseModel: string;
    seed: number;
    steps: number;
    guidanceScale: number;
    quantization: string;
    loraName?: string;
    loraScale?: number;
    width: number;
    height: number;
  } | null>(null);

  /* ── Carrega modelos LoRA de difusão ── */
  useEffect(() => {
    let active = true;
    listModels()
      .then((res) => {
        if (active) {
          setModels(res.items.filter((m) => m.engine === "diffusion"));
          setLoadingModels(false);
        }
      })
      .catch(() => {
        if (active) setLoadingModels(false);
      });
    return () => {
      active = false;
    };
  }, []);

  /* ── Modelos LoRA mapeados para Select ── */
  const loraOptions = useMemo<SelectOption<string>[]>(() => {
    const opts: SelectOption<string>[] = [
      {
        value: "",
        label: "Nenhum (Modelo Base Puro)",
        description: "Executa inferência com os pesos originais do base",
      },
    ];
    models.forEach((m) => {
      opts.push({
        value: m.id,
        label: m.name,
        description: `LoRA · ${m.source}${m.model ? ` · ${m.model}` : ""}`,
      });
    });
    return opts;
  }, [models]);

  /* ── Ajusta defaults quando troca o modelo base ── */
  const handleBaseModelChange = useCallback((modelVal: string) => {
    const b = modelVal as "flux-2-klein-4b" | "sdxl" | "sd15";
    setBaseModel(b);
    if (b === "flux-2-klein-4b") {
      setGuidanceScale(3.5);
      setSteps(20);
      setWidth(1024);
      setHeight(1024);
    } else if (b === "sdxl") {
      setGuidanceScale(7.0);
      setSteps(25);
      setWidth(1024);
      setHeight(1024);
      setShowNegative(true);
    } else if (b === "sd15") {
      setGuidanceScale(7.0);
      setSteps(20);
      setWidth(512);
      setHeight(512);
      setShowNegative(true);
    }
  }, []);

  /* ── Randomizar Seed ── */
  const handleRollSeed = useCallback(() => {
    const nextSeed = Math.floor(Math.random() * 10000000);
    setSeed(nextSeed);
    showToast(`Nova semente gerada: ${nextSeed}`, "info");
  }, []);

  /* ── Submissão do Job de Geração ── */
  const handleGenerate = useCallback(
    async (overrideSeed?: number) => {
      if (!prompt.trim()) {
        showToast("Insira uma descrição textual para gerar a imagem.", "error");
        return;
      }
      const effectiveSeed = overrideSeed !== undefined ? overrideSeed : seed;
      const selectedLoRAModel = models.find((m) => m.id === selectedLoRAId);

      submittedParamsRef.current = {
        prompt: prompt.trim(),
        negativePrompt: negativePrompt.trim() ? negativePrompt.trim() : undefined,
        baseModel,
        seed: effectiveSeed,
        steps,
        guidanceScale,
        quantization,
        loraName: selectedLoRAModel?.name,
        loraScale: selectedLoRAModel ? loraScale : undefined,
        width,
        height,
      };

      setSubmitting(true);
      try {
        const res = await startDiffusionGenerateJob({
          baseModel,
          prompt: prompt.trim(),
          negativePrompt: negativePrompt.trim() ? negativePrompt.trim() : undefined,
          width,
          height,
          steps,
          guidanceScale,
          seed: effectiveSeed,
          quantization,
          weights: selectedLoRAId || null,
          loraScale: selectedLoRAId ? loraScale : undefined,
          orchestratorId: selectedOrchestratorId,
        });

        setActiveJobId(res.jobId);
        showToast(
          res.queuePosition
            ? `Job de difusão enfileirado (posição ${res.queuePosition})`
            : "Inferência de difusão iniciada no nó!",
          "info"
        );

        // Se a seed não estiver travada, prepara a próxima seed aleatória
        if (!isLockedSeed && overrideSeed === undefined) {
          setSeed(Math.floor(Math.random() * 10000000));
        }
      } catch (err) {
        if (err instanceof ApiError) {
          showToast(diffusionGenerateErrorMessage(err.code), "error");
        } else {
          showToast("Falha ao comunicar com o servidor.", "error");
        }
      } finally {
        setSubmitting(false);
      }
    },
    [
      baseModel,
      prompt,
      negativePrompt,
      width,
      height,
      steps,
      guidanceScale,
      seed,
      quantization,
      selectedLoRAId,
      loraScale,
      selectedOrchestratorId,
      isLockedSeed,
    ]
  );

  /* ── Polling do Job Ativo ── */
  useEffect(() => {
    if (!activeJobId) return;

    let cancelled = false;

    const checkJob = async () => {
      try {
        const job = await getJob(activeJobId);
        if (cancelled) return;
        setActiveJob(job);

        if (job.status === "done") {
          const imgUrl = await getGeneratedImageUrl(job.id);
          if (imgUrl && !cancelled) {
            const params = submittedParamsRef.current;
            const newItem: GeneratedImageItem = {
              jobId: job.id,
              imageUrl: imgUrl,
              prompt: params?.prompt || prompt,
              negativePrompt: params?.negativePrompt || (negativePrompt.trim() ? negativePrompt.trim() : undefined),
              baseModel: params?.baseModel || job.model || baseModel,
              seed: params?.seed ?? seed,
              steps: params?.steps ?? steps,
              guidanceScale: params?.guidanceScale ?? guidanceScale,
              quantization: params?.quantization || quantization,
              loraName: params?.loraName,
              loraScale: params?.loraScale,
              width: params?.width || width,
              height: params?.height || height,
              createdAt: job.finishedAt || new Date().toISOString(),
            };

            setCurrentDisplayItem(newItem);
            setHistory((prev) => [newItem, ...prev.filter((h) => h.jobId !== newItem.jobId)]);
            showToast("Imagem gerada com sucesso!", "success");
          }
          setActiveJobId(null);
        } else if (job.status === "failed" || job.status === "cancelled") {
          setActiveJobId(null);
          showToast(job.error || "A geração de imagem falhou.", "error");
        }
      } catch {
        // Ignora falhas esparsas de polling
      }
    };

    checkJob();
    pollingRef.current = setInterval(checkJob, 2000);

    return () => {
      cancelled = true;
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
        pollingRef.current = null;
      }
    };
  }, [
    activeJobId,
    baseModel,
    prompt,
    negativePrompt,
    width,
    height,
    steps,
    guidanceScale,
    seed,
    quantization,
    selectedLoRAId,
    loraScale,
    models,
  ]);

  /* ── Atalho Ctrl+Enter para disparar geração ── */
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        if (!submitting && !activeJobId && prompt.trim()) {
          e.preventDefault();
          handleGenerate();
        }
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleGenerate, submitting, activeJobId, prompt]);

  /* ── Copiar Prompt ── */
  const handleCopyPrompt = useCallback(() => {
    if (!currentDisplayItem) return;
    navigator.clipboard.writeText(currentDisplayItem.prompt);
    setCopiedPrompt(true);
    showToast("Prompt copiado para a área de transferência!", "success");
    setTimeout(() => setCopiedPrompt(false), 2000);
  }, [currentDisplayItem]);

  /* ── Download Imagem ── */
  const handleDownload = useCallback(() => {
    if (!currentDisplayItem) return;
    const a = document.createElement("a");
    a.href = currentDisplayItem.imageUrl;
    a.download = `hephaestus-${currentDisplayItem.baseModel}-seed-${currentDisplayItem.seed}.png`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  }, [currentDisplayItem]);

  return (
    <div className="grid grid-cols-1 lg:grid-cols-[380px_1fr] gap-6 items-start">
      {/* ──────────────────────────────────────────────────────────
          Coluna Esquerda: Controles de Geração (Dark-Only Vidro)
          ────────────────────────────────────────────────────────── */}
      <div className="space-y-5">
        <GlassCard className="p-5 border-white/10 space-y-4">
          <div className="flex items-center justify-between pb-3 border-b border-white/5">
            <div className="flex items-center gap-2">
              <IconSparkles className="w-4 h-4 text-brand-400" />
              <h2 className="text-xs font-semibold uppercase tracking-wider text-zinc-300">
                Parâmetros de Geração
              </h2>
            </div>
            <span className="text-[10px] font-mono text-zinc-400 bg-zinc-800/80 px-2 py-0.5 rounded border border-white/5">
              Diffusers Engine
            </span>
          </div>

          {/* Modelo Base */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-zinc-300">Modelo Base</label>
            <Select
              options={BASE_MODEL_OPTIONS}
              value={baseModel}
              onChange={handleBaseModelChange}
              disabled={submitting || !!activeJobId}
            />
          </div>

          {/* Pesos LoRA (Opcional) */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-300">Adaptador LoRA (Opcional)</label>
              {selectedLoRAId && (
                <span className="text-[10px] font-mono text-brand-400 bg-brand-500/10 px-1.5 py-0.2 rounded border border-brand-500/20">
                  LoRA Ativo
                </span>
              )}
            </div>
            <Select
              options={loraOptions}
              value={selectedLoRAId}
              onChange={setSelectedLoRAId}
              disabled={loadingModels || submitting || !!activeJobId}
              placeholder={loadingModels ? "Carregando catálogo..." : "Nenhum (Base Puro)"}
            />
          </div>

          {/* Força do LoRA (apenas se selecionado) */}
          {selectedLoRAId && (
            <div className="space-y-1.5 p-3 rounded-lg border border-brand-500/20 bg-brand-500/[0.04]">
              <div className="flex items-center justify-between">
                <label className="text-xs font-medium text-brand-300">Força do LoRA (Scale)</label>
                <span className="text-xs font-mono font-semibold text-brand-400">
                  {loraScale.toFixed(2)}x
                </span>
              </div>
              <input
                type="range"
                min={0}
                max={2}
                step={0.05}
                value={loraScale}
                onChange={(e) => setLoraScale(parseFloat(e.target.value))}
                disabled={submitting || !!activeJobId}
                className="w-full accent-brand-500 bg-zinc-800 rounded-lg cursor-pointer h-1.5"
              />
            </div>
          )}

          {/* Prompt Positivo */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-300">Prompt Textual</label>
              <span className="text-[10px] font-mono text-zinc-400">{prompt.length}/4000</span>
            </div>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              disabled={submitting || !!activeJobId}
              placeholder="Ex: a high-tech glowing forge with molten metal, intricate lasers, volumetric lighting, photorealistic 8k..."
              rows={4}
              className="w-full px-3 py-2.5 rounded-lg border border-white/10 bg-zinc-950/80 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-1 focus:ring-brand-500/50 focus:border-brand-500/50 transition-all resize-none"
            />
          </div>

          {/* Prompt Negativo Colapsável */}
          <div className="space-y-1.5">
            {!showNegative ? (
              <button
                type="button"
                onClick={() => setShowNegative(true)}
                className="text-[11px] text-zinc-400 hover:text-zinc-200 flex items-center gap-1 transition-colors"
              >
                <span>+ Adicionar Prompt Negativo</span>
              </button>
            ) : (
              <div className="space-y-1.5 pt-1">
                <div className="flex items-center justify-between">
                  <label className="text-xs font-medium text-zinc-400">Prompt Negativo</label>
                  <button
                    type="button"
                    onClick={() => {
                      setShowNegative(false);
                      setNegativePrompt("");
                    }}
                    className="text-[10px] text-zinc-400 hover:text-zinc-300"
                  >
                    Ocultar
                  </button>
                </div>
                <textarea
                  value={negativePrompt}
                  onChange={(e) => setNegativePrompt(e.target.value)}
                  disabled={submitting || !!activeJobId}
                  placeholder="Ex: blurry, distorted anatomy, text, low quality, artifacts..."
                  rows={2}
                  className="w-full px-3 py-2 rounded-lg border border-white/10 bg-zinc-950/80 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:ring-1 focus:ring-brand-500/50 focus:border-brand-500/50 transition-all resize-none"
                />
              </div>
            )}
          </div>

          {/* Dimensões / Proporção de Tela */}
          <div className="space-y-2 pt-2 border-t border-white/5">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-300">Proporção & Resolução</label>
              <span className="text-xs font-mono text-zinc-400">
                {width} × {height}
              </span>
            </div>
            <div className="grid grid-cols-3 gap-1.5">
              {ASPECT_RATIO_PRESETS.map((preset) => {
                const isActive = width === preset.width && height === preset.height;
                return (
                  <button
                    key={preset.label}
                    type="button"
                    onClick={() => {
                      setWidth(preset.width);
                      setHeight(preset.height);
                    }}
                    disabled={submitting || !!activeJobId}
                    className={`px-2 py-1.5 rounded-lg text-[11px] font-medium transition-all text-center border ${
                      isActive
                        ? "bg-brand-500/20 border-brand-500/40 text-brand-300 font-semibold"
                        : "bg-zinc-900/80 border-white/5 text-zinc-400 hover:text-zinc-200 hover:border-white/10"
                    }`}
                  >
                    {preset.label}
                  </button>
                );
              })}
            </div>
          </div>

          {/* Hiperparâmetros: Steps & CFG */}
          <div className="grid grid-cols-2 gap-3 pt-2">
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label className="text-[11px] text-zinc-400">Steps</label>
                <span className="text-[11px] font-mono font-semibold text-zinc-200">{steps}</span>
              </div>
              <input
                type="range"
                min={4}
                max={50}
                step={1}
                value={steps}
                onChange={(e) => setSteps(parseInt(e.target.value, 10))}
                disabled={submitting || !!activeJobId}
                className="w-full accent-brand-500 bg-zinc-800 rounded-lg cursor-pointer h-1.5"
              />
            </div>

            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label className="text-[11px] text-zinc-400">CFG Scale</label>
                <span className="text-[11px] font-mono font-semibold text-zinc-200">
                  {guidanceScale.toFixed(1)}
                </span>
              </div>
              <input
                type="range"
                min={1.0}
                max={15.0}
                step={0.5}
                value={guidanceScale}
                onChange={(e) => setGuidanceScale(parseFloat(e.target.value))}
                disabled={submitting || !!activeJobId}
                className="w-full accent-brand-500 bg-zinc-800 rounded-lg cursor-pointer h-1.5"
              />
            </div>
          </div>

          {/* Semente (Seed) com botão Randomize */}
          <div className="space-y-1.5 pt-1">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-300">Semente (Seed)</label>
              <button
                type="button"
                onClick={() => setIsLockedSeed(!isLockedSeed)}
                className={`text-[10px] font-mono px-1.5 py-0.5 rounded border transition-colors ${
                  isLockedSeed
                    ? "bg-amber-500/10 border-amber-500/30 text-amber-300"
                    : "bg-zinc-800/80 border-white/5 text-zinc-400 hover:text-zinc-200"
                }`}
              >
                {isLockedSeed ? "Travar Semente [On]" : "Auto-Random [Off]"}
              </button>
            </div>
            <div className="flex items-center gap-2">
              <input
                type="number"
                value={seed}
                onChange={(e) => setSeed(parseInt(e.target.value, 10) || 0)}
                disabled={submitting || !!activeJobId}
                className="flex-1 px-3 py-1.5 rounded-lg border border-white/10 bg-zinc-950/80 font-mono text-xs text-zinc-200 focus:outline-none focus:ring-1 focus:ring-brand-500/50"
              />
              <button
                type="button"
                onClick={handleRollSeed}
                disabled={submitting || !!activeJobId}
                title="Rolar nova semente aleatória"
                className="p-2 rounded-lg border border-white/10 bg-zinc-900/80 hover:bg-zinc-800 text-zinc-300 transition-colors"
              >
                <IconDice className="w-4 h-4" />
              </button>
            </div>
          </div>

          {/* Quantização de VRAM */}
          <div className="space-y-1.5 pt-1">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium text-zinc-300">Quantização de VRAM</label>
              <span className="text-[10px] font-mono text-brand-400">
                {quantization === "4bit" ? "~8-10 GB" : quantization === "8bit" ? "~12-14 GB" : "~20 GB+"}
              </span>
            </div>
            <Select
              options={QUANTIZATION_OPTIONS}
              value={quantization}
              onChange={(v) => setQuantization(v as "4bit" | "8bit" | "none")}
              disabled={submitting || !!activeJobId}
            />
          </div>

          {/* Seletor de Nó */}
          <div className="space-y-1.5 pt-2 border-t border-white/5">
            <NodeSelect
              value={selectedOrchestratorId}
              onChange={setSelectedOrchestratorId}
              disabled={submitting || !!activeJobId}
            />
          </div>

          {/* CTA Principal de Execução */}
          <div className="pt-2">
            <button
              type="button"
              onClick={() => handleGenerate()}
              disabled={submitting || !!activeJobId || !prompt.trim()}
              className="w-full relative group overflow-hidden rounded-xl border border-brand-500/30 bg-brand-500/[0.14] hover:bg-brand-500/[0.22] active:bg-brand-500/[0.28] text-white px-4 py-3 text-xs font-semibold tracking-wide transition-all shadow-[inset_0_1px_0_rgba(255,255,255,0.12)] disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
            >
              {submitting || !!activeJobId ? (
                <>
                  <IconRefresh className="w-4 h-4 animate-spin text-brand-300" />
                  <span>
                    {activeJob?.status === "running"
                      ? "Gerando Imagem na GPU..."
                      : "Enfileirando job..."}
                  </span>
                </>
              ) : (
                <>
                  <IconPlay className="w-4 h-4 fill-brand-300 text-brand-300" />
                  <span>Gerar Imagem</span>
                  <kbd className="hidden sm:inline-block px-1.5 py-0.5 rounded border border-white/10 bg-white/[0.06] text-zinc-400 font-mono text-[10px]">
                    Ctrl+Enter
                  </kbd>
                </>
              )}
            </button>
          </div>
        </GlassCard>
      </div>

      {/* ──────────────────────────────────────────────────────────
          Coluna Direita: Canvas & Histórico de Sessão
          ────────────────────────────────────────────────────────── */}
      <div className="space-y-5">
        {/* Status de Job em Andamento */}
        {activeJob && (activeJob.status === "queued" || activeJob.status === "running") && (
          <GlassCard className="p-4 border-brand-500/30 bg-brand-500/[0.04] flex items-center justify-between animate-pulse">
            <div className="flex items-center gap-3">
              <IconRefresh className="w-5 h-5 text-brand-400 animate-spin" />
              <div>
                <p className="text-xs font-semibold text-zinc-200">
                  {activeJob.status === "running"
                    ? "Geração de imagem em execução na GPU"
                    : "Job aguardando na fila de execução"}
                </p>
                <p className="text-[11px] font-mono text-zinc-400">
                  Job ID: {activeJob.id} · Modelo: {activeJob.model || baseModel}
                </p>
              </div>
            </div>
            <span className="text-xs font-mono font-semibold px-2.5 py-1 rounded bg-brand-500/20 text-brand-300 border border-brand-500/30">
              {activeJob.status.toUpperCase()}
            </span>
          </GlassCard>
        )}

        {/* Canvas Principal */}
        {currentDisplayItem ? (
          <GlassCard className="p-5 border-white/10 space-y-4">
            {/* Toolbar Superior do Canvas */}
            <div className="flex flex-wrap items-center justify-between gap-2 pb-3 border-b border-white/5">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-zinc-200">Visualizador</span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-800 border border-white/5 text-zinc-400">
                  {currentDisplayItem.width} × {currentDisplayItem.height}
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                  {currentDisplayItem.baseModel}
                </span>
              </div>

              <div className="flex items-center gap-1.5">
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={handleCopyPrompt}
                  title="Copiar prompt"
                  className="text-xs text-zinc-300 hover:text-white"
                >
                  {copiedPrompt ? <IconCheck className="w-3.5 h-3.5 text-brand-400" /> : <IconCopy className="w-3.5 h-3.5" />}
                  <span className="ml-1">Copiar Prompt</span>
                </Button>

                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => handleGenerate(currentDisplayItem.seed)}
                  disabled={submitting || !!activeJobId}
                  title="Re-gerar usando a mesma semente"
                  className="text-xs text-zinc-300 hover:text-white"
                >
                  <IconRefresh className="w-3.5 h-3.5" />
                  <span className="ml-1">Mesma Seed</span>
                </Button>

                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => handleGenerate(Math.floor(Math.random() * 10000000))}
                  disabled={submitting || !!activeJobId}
                  title="Gerar com nova semente"
                  className="text-xs text-zinc-300 hover:text-white"
                >
                  <IconDice className="w-3.5 h-3.5" />
                  <span className="ml-1">Nova Variação</span>
                </Button>

                <Button
                  size="sm"
                  variant="primary"
                  onClick={handleDownload}
                  title="Baixar imagem PNG"
                  className="text-xs"
                >
                  <IconDownload className="w-3.5 h-3.5" />
                  <span className="ml-1">Baixar PNG</span>
                </Button>
              </div>
            </div>

            {/* Imagem em Destaque com visualizador óptico */}
            <div className="relative group rounded-xl overflow-hidden bg-black/40 border border-white/5 flex items-center justify-center min-h-[420px]">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={currentDisplayItem.imageUrl}
                alt={currentDisplayItem.prompt}
                className="max-h-[640px] w-auto max-w-full object-contain rounded-lg shadow-2xl transition-transform duration-300 group-hover:scale-[1.01]"
              />

              {/* Botão de Zoom / Lightbox */}
              <button
                type="button"
                onClick={() => setLightboxOpen(true)}
                className="absolute top-3 right-3 p-2 rounded-lg bg-zinc-900/80 hover:bg-zinc-800 text-white/80 hover:text-white border border-white/10 backdrop-blur-md opacity-0 group-hover:opacity-100 transition-all"
                title="Ampliar Imagem"
              >
                <IconZoomIn className="w-4 h-4" />
              </button>
            </div>

            {/* Metadados Técnicos Detalhados */}
            <div className="p-3.5 rounded-xl border border-white/5 bg-zinc-950/60 space-y-2">
              <p className="text-xs text-zinc-300 italic font-sans leading-relaxed">
                &ldquo;{currentDisplayItem.prompt}&rdquo;
              </p>

              {currentDisplayItem.negativePrompt && (
                <p className="text-[11px] text-zinc-400">
                  <span className="font-semibold text-zinc-500 uppercase text-[9px] tracking-wider mr-1.5">
                    Negativo:
                  </span>
                  {currentDisplayItem.negativePrompt}
                </p>
              )}

              <div className="flex flex-wrap items-center gap-2 pt-1">
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  Seed: <span className="text-zinc-200 font-semibold">{currentDisplayItem.seed}</span>
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  Steps: <span className="text-zinc-200 font-semibold">{currentDisplayItem.steps}</span>
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  CFG: <span className="text-zinc-200 font-semibold">{currentDisplayItem.guidanceScale.toFixed(1)}</span>
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  Quant: <span className="text-zinc-200 font-semibold">{currentDisplayItem.quantization}</span>
                </span>
                {currentDisplayItem.loraName && (
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                    LoRA: {currentDisplayItem.loraName} ({currentDisplayItem.loraScale?.toFixed(2)}x)
                  </span>
                )}
              </div>
            </div>
          </GlassCard>
        ) : (
          <GlassCard className="p-12 border-white/10 flex flex-col items-center justify-center text-center space-y-3 min-h-[420px]">
            <div className="w-12 h-12 rounded-2xl border border-brand-500/20 bg-brand-500/[0.08] flex items-center justify-center text-brand-400 shadow-inner">
              <IconSparkles className="w-6 h-6" />
            </div>
            <h3 className="text-sm font-semibold text-zinc-200">Pronto para gerar imagens</h3>
            <p className="text-xs text-zinc-400 max-w-sm">
              Configure o modelo base, selecione um LoRA treinado e descreva a imagem desejada na coluna ao lado. Clique em &ldquo;Gerar Imagem&rdquo; ou pressione <kbd className="text-[10px] font-mono px-1 py-0.5 rounded bg-zinc-800 border border-white/10 text-zinc-300">Ctrl+Enter</kbd>.
            </p>
          </GlassCard>
        )}

        {/* Galeria de Histórico da Sessão */}
        {history.length > 0 && (
          <GlassCard className="p-4 border-white/10 space-y-3">
            <div className="flex items-center justify-between">
              <h4 className="text-xs font-semibold uppercase tracking-wider text-zinc-300 flex items-center gap-1.5">
                <span>Gerações da Sessão</span>
                <span className="text-[10px] font-mono text-zinc-400 bg-zinc-800 px-1.5 py-0.2 rounded">
                  {history.length}
                </span>
              </h4>
            </div>

            <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-6 gap-2.5 overflow-x-auto pb-1">
              {history.map((item) => {
                const isSelected = currentDisplayItem?.jobId === item.jobId;
                return (
                  <button
                    key={item.jobId}
                    type="button"
                    onClick={() => setCurrentDisplayItem(item)}
                    className={`group relative rounded-lg overflow-hidden border aspect-square transition-all ${
                      isSelected
                        ? "border-brand-500 ring-2 ring-brand-500/30 scale-[1.02]"
                        : "border-white/10 hover:border-white/20 opacity-70 hover:opacity-100"
                    }`}
                  >
                    {/* eslint-disable-next-line @next/next/no-img-element */}
                    <img
                      src={item.imageUrl}
                      alt={item.prompt}
                      className="w-full h-full object-cover"
                    />
                    <div className="absolute inset-0 bg-gradient-to-t from-black/80 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity flex items-end p-1.5">
                      <p className="text-[9px] font-mono text-zinc-200 truncate w-full text-left">
                        {item.baseModel} · #{item.seed}
                      </p>
                    </div>
                  </button>
                );
              })}
            </div>
          </GlassCard>
        )}
      </div>

      {/* ──────────────────────────────────────────────────────────
          Modal Lightbox em Tela Cheia
          ────────────────────────────────────────────────────────── */}
      {lightboxOpen && currentDisplayItem && (
        <div
          className="fixed inset-0 z-50 bg-black/90 backdrop-blur-xl flex items-center justify-center p-4 sm:p-8 animate-fadeIn"
          onClick={() => setLightboxOpen(false)}
        >
          <div
            className="relative max-w-5xl w-full max-h-[90vh] flex flex-col items-center justify-center"
            onClick={(e) => e.stopPropagation()}
          >
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={currentDisplayItem.imageUrl}
              alt={currentDisplayItem.prompt}
              className="max-h-[80vh] w-auto max-w-full object-contain rounded-xl shadow-2xl border border-white/10"
            />
            <div className="mt-4 flex items-center justify-between w-full max-w-2xl px-4 py-2 rounded-xl bg-zinc-900/80 border border-white/10 backdrop-blur-md">
              <p className="text-xs text-zinc-300 truncate max-w-md font-sans italic">
                &ldquo;{currentDisplayItem.prompt}&rdquo;
              </p>
              <div className="flex items-center gap-2">
                <Button size="sm" variant="primary" onClick={handleDownload}>
                  <IconDownload className="w-3.5 h-3.5 mr-1" />
                  Baixar PNG
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setLightboxOpen(false)}>
                  Fechar
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
