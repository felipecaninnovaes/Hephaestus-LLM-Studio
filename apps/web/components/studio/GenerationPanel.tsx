"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  IconDice,
  IconDownload,
  IconImage,
  IconPlay,
  IconRefresh,
  IconSliders,
  IconSparkles,
  IconStop,
  IconZap,
  IconZoomIn,
} from "@/components/icons";
import {
  Button,
  EmptyState,
  GlassCard,
  Kbd,
  Modal,
  SegmentedControl,
  Select,
  Slider,
  type SelectOption,
  showToast,
} from "@/components/ui";
import { listJobs, getJob, getJobArtifacts } from "@/lib/jobs";
import { listModels } from "@/lib/models";
import {
  getGeneratedImageUrl,
  getGeneratedBatchResults,
  startDiffusionGenerateJob,
} from "@/lib/playground";
import type { Job, Model, LoraRef, DiffusionGenerateJobRequest } from "@/types/studio";
import { diffusionGenerateErrorMessage } from "@/types/studio";
import { ApiError } from "@/lib/api";
import NodeSelect from "@/components/studio/NodeSelect";
import { LoRAEditor } from "@/components/studio/LoRAEditor";
import { useJobTelemetry } from "@/hooks/useJobTelemetry";
import { JobProgressLive } from "@/components/studio/JobProgressLive";

/* ── Tipos internos ── */

interface GeneratedImageItem {
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
  distilled?: boolean;
  loras: LoraRef[];
  width: number;
  height: number;
  batchIndex?: number;
  createdAt: string;
}

/* ── Constantes ── */

const BASE_MODEL_OPTIONS: SelectOption<string>[] = [
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

const ASPECT_RATIO_PRESETS = [
  { label: "1:1", width: 1024, height: 1024 },
  { label: "16:9", width: 1344, height: 768 },
  { label: "9:16", width: 768, height: 1344 },
  { label: "4:3", width: 1152, height: 864 },
  { label: "Eco 512", width: 512, height: 512 },
];

const QUANTIZATION_OPTIONS: SelectOption<string>[] = [
  {
    value: "4bit",
    label: "4-bit NF4",
    description: "~8–10 GB VRAM",
  },
  {
    value: "8bit",
    label: "8-bit BnB",
    description: "~12–14 GB VRAM",
  },
  {
    value: "none",
    label: "FP16",
    description: "~18+ GB VRAM",
  },
];

/* ═══════════════════════════════════════════════════════════════════
   GenerationPanel — painel de geração v3 (G.7 fix — Vidro Óptico)
   Desktop: 2 colunas (controles 320-384px glass-card | resultado flex-1)
   Mobile (<768px): 1 coluna, controles colapsáveis em glass-card
   ═══════════════════════════════════════════════════════════════════ */

export default function GenerationPanel() {
  /* ── Models data ── */
  const [allModels, setAllModels] = useState<Model[]>([]);
  const [loadingModels, setLoadingModels] = useState(true);

  /* ── Form state ── */
  const [baseModel, setBaseModel] = useState<"flux-2-klein-4b" | "sdxl" | "sd15">("flux-2-klein-4b");
  const [customModelId, setCustomModelId] = useState<string>("");
  const [modelMode, setModelMode] = useState<"preset" | "custom">("preset");
  const [distilled, setDistilled] = useState(true);
  const [loras, setLoras] = useState<LoraRef[]>([]);
  const [prompt, setPrompt] = useState("");
  const [negativePrompt, setNegativePrompt] = useState("");
  const [showNegative, setShowNegative] = useState(false);
  const [width, setWidth] = useState(1024);
  const [height, setHeight] = useState(1024);
  const [steps, setSteps] = useState(4);
  const [guidanceScale, setGuidanceScale] = useState(1.0);
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 1000000));
  const [isLockedSeed, setIsLockedSeed] = useState(false);
  const [quantization, setQuantization] = useState<"4bit" | "8bit" | "none">("4bit");
  const [batchSize, setBatchSize] = useState(1);
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);
  const [paramsOpen, setParamsOpen] = useState(true);

  /* ── Execution state ── */
  const [submitting, setSubmitting] = useState(false);
  const [activeJobId, setActiveJobId] = useState<string | null>(null);
  const [activeJob, setActiveJob] = useState<Job | null>(null);
  const telemetry = useJobTelemetry(activeJobId);
  const [currentDisplayItem, setCurrentDisplayItem] = useState<GeneratedImageItem | null>(null);
  const [batchResults, setBatchResults] = useState<GeneratedImageItem[]>([]);
  const [history, setHistory] = useState<GeneratedImageItem[]>([]);
  const [lightboxOpen, setLightboxOpen] = useState(false);

  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const submittedParamsRef = useRef<Omit<GeneratedImageItem, "imageUrl" | "thumbUrl"> | null>(null);

  /* ── Derived model lists ── */
  const diffusionModels = useMemo(
    () => allModels.filter((m) => m.engine === "diffusion"),
    [allModels],
  );
  const loraModels = useMemo(
    () => diffusionModels.filter((m) => m.kind === "lora" || !m.kind),
    [diffusionModels],
  );
  const checkpointModels = useMemo(
    () => diffusionModels.filter((m) => m.kind === "checkpoint"),
    [diffusionModels],
  );

  const checkpointOptions = useMemo<SelectOption<string>[]>(() => {
    return checkpointModels.map((m) => ({
      value: m.id,
      label: m.name,
      description: `checkpoint${m.arch ? ` ${m.arch}` : ""} · ${m.source}`,
    }));
  }, [checkpointModels]);

  /* ── Fetch models ── */
  useEffect(() => {
    let active = true;
    listModels()
      .then((res) => {
        if (active) {
          setAllModels(res.items.filter((m) => m.engine === "diffusion"));
          setLoadingModels(false);
        }
      })
      .catch(() => {
        if (active) setLoadingModels(false);
      });
    return () => { active = false; };
  }, []);

  /* ── Auto-adjust when base model changes ── */
  const handleBaseModelChange = useCallback((modelVal: string) => {
    const b = modelVal as "flux-2-klein-4b" | "sdxl" | "sd15";
    setBaseModel(b);
    if (b === "flux-2-klein-4b") {
      setGuidanceScale(distilled ? 1.0 : 3.5);
      setSteps(distilled ? 4 : 20);
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
  }, [distilled]);

  /* ── Variant change (FLUX only) ── */
  const handleVariantChange = useCallback((val: string) => {
    const isDistilled = val === "distilled";
    setDistilled(isDistilled);
    if (isDistilled) {
      setSteps((s) => (s > 8 ? 4 : s));
      setGuidanceScale((g) => (g > 2.0 ? 1.0 : g));
    } else {
      setSteps((s) => (s < 12 ? 20 : s));
      setGuidanceScale((g) => (g <= 1.5 ? 3.5 : g));
    }
  }, []);

  /* ── Roll seed ── */
  const handleRollSeed = useCallback(() => {
    setSeed(Math.floor(Math.random() * 10000000));
  }, []);

  /* ── Submit generation ── */
  const handleGenerate = useCallback(
    async (overrideSeed?: number) => {
      if (!prompt.trim()) {
        showToast("Insira uma descrição para gerar.", "error");
        return;
      }
      const effectiveSeed = overrideSeed !== undefined ? overrideSeed : seed;
      const isDistilledActive = baseModel === "flux-2-klein-4b" ? distilled : false;

      // Build request: XOR baseModel / customModelId
      const request: DiffusionGenerateJobRequest = {
        prompt: prompt.trim(),
        negativePrompt: negativePrompt.trim() || undefined,
        width,
        height,
        steps,
        guidanceScale,
        seed: effectiveSeed,
        quantization,
        distilled: isDistilledActive,
        batchSize,
        loras: loras.filter((l) => l.modelId).length > 0
          ? loras.filter((l) => l.modelId)
          : undefined,
        orchestratorId: selectedOrchestratorId,
      };

      if (modelMode === "custom" && customModelId) {
        request.customModelId = customModelId;
      } else {
        request.baseModel = baseModel;
      }

      // Store params for history
      submittedParamsRef.current = {
        jobId: "",
        prompt: prompt.trim(),
        negativePrompt: negativePrompt.trim() || undefined,
        baseModel: modelMode === "custom" ? `custom:${customModelId}` : baseModel,
        customModelId: modelMode === "custom" ? customModelId : undefined,
        seed: effectiveSeed,
        steps,
        guidanceScale,
        quantization,
        distilled: isDistilledActive,
        loras: loras.filter((l) => l.modelId),
        width,
        height,
        createdAt: new Date().toISOString(),
      };

      setSubmitting(true);
      try {
        const res = await startDiffusionGenerateJob(request);
        setActiveJobId(res.jobId);

        showToast(
          res.queuePosition
            ? `Job enfileirado (posição ${res.queuePosition})`
            : "Geração iniciada!",
          "info",
        );

        // Roll seed if unlocked
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
      baseModel, modelMode, customModelId, prompt, negativePrompt, width, height,
      steps, guidanceScale, seed, quantization, distilled, batchSize, loras,
      selectedOrchestratorId, isLockedSeed,
    ],
  );

  /* ── Ctrl+Enter shortcut ── */
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

  /* ── Poll active job ── */
  useEffect(() => {
    if (!activeJobId) return;
    let cancelled = false;

    const checkJob = async () => {
      try {
        const job = await getJob(activeJobId);
        if (cancelled) return;
        setActiveJob(job);

        if (job.status === "done") {
          // Get batch results
          const batchItems = await getGeneratedBatchResults(job.id);
          if (cancelled) return;

          const params = submittedParamsRef.current;
          const newItems: GeneratedImageItem[] = batchItems.map((item, idx) => ({
            jobId: job.id,
            imageUrl: item.imageUrl,
            thumbUrl: item.thumbUrl,
            prompt: params?.prompt || prompt,
            negativePrompt: params?.negativePrompt,
            baseModel: params?.baseModel || baseModel,
            customModelId: params?.customModelId,
            seed: (params?.seed ?? seed) + idx,
            steps: params?.steps ?? steps,
            guidanceScale: params?.guidanceScale ?? guidanceScale,
            quantization: params?.quantization || quantization,
            distilled: params?.distilled,
            loras: params?.loras || [],
            width: params?.width || width,
            height: params?.height || height,
            batchIndex: idx,
            createdAt: job.finishedAt || new Date().toISOString(),
          }));

          if (newItems.length > 0) {
            setBatchResults(newItems);
            setCurrentDisplayItem(newItems[0]);
            setHistory((prev) => [
              ...newItems.filter((n) => !prev.some((h) => h.jobId === n.jobId && h.batchIndex === n.batchIndex)),
              ...prev,
            ]);
          } else {
            // Fallback: single image
            const imgUrl = await getGeneratedImageUrl(job.id);
            if (imgUrl && !cancelled) {
              const singleItem: GeneratedImageItem = {
                jobId: job.id,
                imageUrl: imgUrl,
                prompt: params?.prompt || prompt,
                negativePrompt: params?.negativePrompt,
                baseModel: params?.baseModel || baseModel,
                customModelId: params?.customModelId,
                seed: params?.seed ?? seed,
                steps: params?.steps ?? steps,
                guidanceScale: params?.guidanceScale ?? guidanceScale,
                quantization: params?.quantization || quantization,
                distilled: params?.distilled,
                loras: params?.loras || [],
                width: params?.width || width,
                height: params?.height || height,
                createdAt: job.finishedAt || new Date().toISOString(),
              };
              setBatchResults([singleItem]);
              setCurrentDisplayItem(singleItem);
              setHistory((prev) => [singleItem, ...prev.filter((h) => h.jobId !== singleItem.jobId)]);
            }
          }

          showToast("Geração concluída!", "success");
          setActiveJobId(null);
        } else if (job.status === "failed" || job.status === "cancelled") {
          setActiveJobId(null);
          showToast(job.error || "A geração falhou.", "error");
        }
      } catch {
        // ignore sparse polling failures
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
  }, [activeJobId]);

  /* ── Download handler ── */
  const handleDownload = useCallback(() => {
    if (!currentDisplayItem) return;
    const a = document.createElement("a");
    a.href = currentDisplayItem.imageUrl;
    a.download = `geracao-${currentDisplayItem.baseModel}-seed${currentDisplayItem.seed}.png`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
  }, [currentDisplayItem]);

  const isBusy = submitting || !!activeJobId;

  return (
    <div className="flex h-full min-h-0 flex-col lg:flex-row">
      {/* ═══════════════════════════════════════════════════════════════
          COLUNA ESQUERDA — CONTROLES (320-384px, scrollável)
          Vidro Óptico Nível 1 (glass-card) — NUNCA bg translúcido caseiro
          ═══════════════════════════════════════════════════════════════ */}
      <div className="w-full shrink-0 lg:w-[384px] lg:border-r lg:border-white/5 lg:overflow-y-auto glass-card rounded-2xl">
        {/* Mobile: toggle colapsável com glass-card; Desktop: sempre visível */}
        <div className="lg:hidden">
          <button
            type="button"
            onClick={() => setParamsOpen((p) => !p)}
            className="glass-card flex w-full items-center justify-between px-4 py-3 text-sm font-semibold text-zinc-200"
          >
            <span className="flex items-center gap-2">
              <IconSliders className="size-4 text-brand-400" />
              Parâmetros
            </span>
            <span className="font-mono text-[10px] text-zinc-400">
              {paramsOpen ? "Ocultar" : "Mostrar"}
            </span>
          </button>
        </div>

        <div className={`${paramsOpen ? "block" : "hidden"} lg:block p-4 md:p-5 space-y-4`}>
          {/* ══ Modelo Base ══ */}
          <div className="space-y-2">
            <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
              Modelo
            </label>
            {/* Toggle preset/custom — SegmentedControl canônico */}
            <SegmentedControl
              options={[
                { id: "preset", label: "Base Padrão" },
                { id: "custom", label: "Custom" },
              ]}
              value={modelMode}
              onChange={(v) => setModelMode(v as "preset" | "custom")}
              ariaLabel="Modo do modelo"
            />

            {modelMode === "preset" ? (
              <Select
                options={BASE_MODEL_OPTIONS}
                value={baseModel}
                onChange={handleBaseModelChange}
                disabled={isBusy}
                size="sm"
              />
            ) : (
              <Select
                options={
                  checkpointOptions.length > 0
                    ? checkpointOptions
                    : [{ value: "", label: "Nenhum checkpoint disponível", disabled: true }]
                }
                value={customModelId}
                onChange={(v) => setCustomModelId(v)}
                disabled={isBusy || checkpointModels.length === 0}
                placeholder={
                  checkpointModels.length === 0
                    ? "Faça upload em Modelos & Pesos"
                    : "Selecione um checkpoint"
                }
                size="sm"
                fontMono
              />
            )}
          </div>

          {/* ══ Variante FLUX (destilada/base) — SegmentedControl canônico ══ */}
          {modelMode === "preset" && baseModel === "flux-2-klein-4b" && (
            <div className="space-y-2 rounded-xl border border-white/8 bg-white/[0.02] p-3">
              <div className="flex items-center justify-between">
                <span className="font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-300">
                  Variante
                </span>
                <span className="font-mono text-[10px] text-brand-400">
                  {distilled ? "4–8 steps · CFG 1.0" : "20+ steps · CFG 3.5+"}
                </span>
              </div>
              <SegmentedControl
                options={[
                  { id: "distilled", label: "Destilado", icon: <IconZap className="size-3.5" /> },
                  { id: "base", label: "Base", icon: <IconSliders className="size-3.5" /> },
                ]}
                value={distilled ? "distilled" : "base"}
                onChange={handleVariantChange}
                ariaLabel="Variante do modelo"
              />
            </div>
          )}

          {/* ══ Multi-LoRA ══ */}
          <LoRAEditor
            value={loras}
            onChange={setLoras}
            loraModels={loraModels}
            disabled={isBusy}
          />

          {/* ══ Prompt ══ */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
                Prompt
              </label>
              <span className="font-mono text-[10px] text-zinc-500">
                {prompt.length}/4000
              </span>
            </div>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value.slice(0, 4000))}
              disabled={isBusy}
              placeholder="Descrição da imagem desejada…"
              rows={3}
              className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition resize-none"
            />
          </div>

          {/* ══ Negative Prompt (colapsável) ══ */}
          {!showNegative ? (
            <button
              type="button"
              onClick={() => setShowNegative(true)}
              className="font-mono text-[11px] text-zinc-400 hover:text-zinc-200 transition-colors"
            >
              + Prompt Negativo
            </button>
          ) : (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label className="font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-300">
                  Prompt Negativo
                </label>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => { setShowNegative(false); setNegativePrompt(""); }}
                >
                  Ocultar
                </Button>
              </div>
              <textarea
                value={negativePrompt}
                onChange={(e) => setNegativePrompt(e.target.value)}
                disabled={isBusy}
                placeholder="Elementos a evitar…"
                rows={2}
                className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition resize-none"
              />
            </div>
          )}

          {/* ══ Resolução — SegmentedControl canônico ══ */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
                Resolução
              </label>
              <span className="font-mono text-[10px] text-zinc-400">{width}×{height}</span>
            </div>
            <SegmentedControl
              options={ASPECT_RATIO_PRESETS.map((p) => ({
                id: `${p.width}x${p.height}`,
                label: p.label,
              }))}
              value={`${width}x${height}`}
              onChange={(v) => {
                const [w, h] = v.split("x").map(Number);
                setWidth(w);
                setHeight(h);
              }}
              ariaLabel="Resolução"
            />
          </div>

          {/* ══ Steps — Slider canônico ══ */}
          <Slider
            label="Steps"
            value={steps}
            onChange={(v) => setSteps(Math.round(v))}
            min={1}
            max={50}
            step={1}
            disabled={isBusy}
            formatValue={(v) => String(Math.round(v))}
          />

          {/* ══ CFG — Slider canônico ══ */}
          <Slider
            label="CFG"
            value={guidanceScale}
            onChange={setGuidanceScale}
            min={1}
            max={15}
            step={0.5}
            disabled={isBusy}
            formatValue={(v) => v.toFixed(1)}
          />

          {/* ══ Seed — Input canônico + Button canônico ══ */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
                Seed
              </label>
              <SegmentedControl
                options={[
                  { id: "auto", label: "Auto" },
                  { id: "locked", label: "Travada" },
                ]}
                value={isLockedSeed ? "locked" : "auto"}
                onChange={(v) => setIsLockedSeed(v === "locked")}
                ariaLabel="Modo seed"
              />
            </div>
            <div className="flex items-center gap-2">
              <input
                type="number"
                value={seed}
                onChange={(e) => setSeed(parseInt(e.target.value, 10) || 0)}
                disabled={isBusy}
                className="flex-1 rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={handleRollSeed}
                disabled={isBusy}
                title="Nova seed aleatória"
              >
                <IconDice className="size-4" />
              </Button>
            </div>
          </div>

          {/* ══ Quantização ══ */}
          <div className="space-y-1.5">
            <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
              Quantização
            </label>
            <Select
              options={QUANTIZATION_OPTIONS}
              value={quantization}
              onChange={(v) => setQuantization(v as "4bit" | "8bit" | "none")}
              disabled={isBusy}
              size="sm"
            />
          </div>

          {/* ══ Batch Size — Slider canônico ══ */}
          <Slider
            label="Quantidade de imagens"
            value={batchSize}
            onChange={(v) => setBatchSize(Math.round(v))}
            min={1}
            max={8}
            step={1}
            disabled={isBusy}
            formatValue={(v) => `${Math.round(v)} · seed +1 por imagem`}
          />

          {/* ══ Nó ══ */}
          <NodeSelect
            value={selectedOrchestratorId}
            onChange={setSelectedOrchestratorId}
            disabled={isBusy}
            size="sm"
          />

          {/* ══ CTA Único ══ */}
          <div className="pt-2">
            <Button
              type="button"
              variant="primary"
              size="lg"
              className="w-full"
              disabled={!prompt.trim() || isBusy}
              onClick={() => handleGenerate()}
            >
              {isBusy ? (
                <>
                  <IconRefresh className="size-4 animate-spin" />
                  {activeJob?.status === "running" ? "Gerando…" : "Enfileirando…"}
                </>
              ) : (
                <>
                  <IconPlay className="size-4" />
                  Gerar
                  <Kbd className="hidden sm:inline-flex ml-1">Ctrl+Enter</Kbd>
                </>
              )}
            </Button>
          </div>
        </div>
      </div>

      {/* ═══════════════════════════════════════════════════════════════
          COLUNA DIREITA — RESULTADO + TELEMETRIA
          ═══════════════════════════════════════════════════════════════ */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden p-4 md:p-5">
        {/* Telemetria live */}
        {activeJobId && (
          <div className="mb-4">
            <JobProgressLive
              phase={telemetry.phase || activeJob?.phase || activeJob?.status}
              phaseMessage={
                telemetry.phaseMessage ||
                activeJob?.phaseMessage ||
                (activeJob?.status === "running" ? "Gerando imagem…" : "Na fila…")
              }
              progress={telemetry.progress || activeJob?.progress || 0}
              vramUsedGb={telemetry.vramUsedGb ?? activeJob?.vramUsedGb}
              step={telemetry.step ?? activeJob?.step}
              totalSteps={telemetry.totalSteps ?? steps}
              isLive={telemetry.isLive}
              isFinished={telemetry.isFinished}
            />
          </div>
        )}

        {/* Canvas / Resultado */}
        {currentDisplayItem ? (
          <GlassCard className="flex-1 min-h-0 flex flex-col p-4 border-white/10">
            {/* Toolbar */}
            <div className="flex flex-wrap items-center justify-between gap-2 pb-3 border-b border-white/5 mb-3">
              <div className="flex items-center gap-2">
                <span className="text-xs font-semibold text-zinc-200">Resultado</span>
                <span className="font-mono text-[10px] px-2 py-0.5 rounded bg-zinc-800 border border-white/5 text-zinc-400">
                  {currentDisplayItem.width}×{currentDisplayItem.height}
                </span>
                <span className="font-mono text-[10px] px-2 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                  seed {currentDisplayItem.seed}
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <Button size="sm" variant="ghost" onClick={handleDownload} title="Baixar PNG">
                  <IconDownload className="size-3.5" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => setLightboxOpen(true)}
                  title="Ampliar"
                >
                  <IconZoomIn className="size-3.5" />
                </Button>
              </div>
            </div>

            {/* Imagem */}
            <div className="flex-1 min-h-0 flex items-center justify-center rounded-xl overflow-hidden bg-black/40 border border-white/5">
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={currentDisplayItem.imageUrl}
                alt={currentDisplayItem.prompt}
                className="max-h-full max-w-full object-contain"
              />
            </div>

            {/* Metadados */}
            <div className="mt-3 p-2.5 rounded-lg border border-white/5 bg-zinc-950/60">
              <p className="text-[11px] text-zinc-300 italic leading-relaxed truncate" title={currentDisplayItem.prompt}>
                &ldquo;{currentDisplayItem.prompt}&rdquo;
              </p>
              <div className="flex flex-wrap items-center gap-1.5 mt-1.5">
                <span className="font-mono text-[9px] px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  {currentDisplayItem.baseModel}
                </span>
                <span className="font-mono text-[9px] px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  steps {currentDisplayItem.steps}
                </span>
                <span className="font-mono text-[9px] px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  CFG {currentDisplayItem.guidanceScale.toFixed(1)}
                </span>
                <span className="font-mono text-[9px] px-1.5 py-0.5 rounded bg-zinc-900 border border-white/5 text-zinc-400">
                  {currentDisplayItem.quantization}
                </span>
                {currentDisplayItem.loras.length > 0 && (
                  <span className="font-mono text-[9px] px-1.5 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300">
                    {currentDisplayItem.loras.length} LoRA(s)
                  </span>
                )}
              </div>
            </div>

            {/* Batch results grid */}
            {batchResults.length > 1 && (
              <div className="mt-3 space-y-2">
                <span className="font-mono text-[10px] text-zinc-400 uppercase tracking-wider">
                  Batch ({batchResults.length} imagens)
                </span>
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                  {batchResults.map((item, idx) => (
                    <button
                      key={`${item.jobId}-${idx}`}
                      type="button"
                      onClick={() => setCurrentDisplayItem(item)}
                      className={`group relative rounded-lg overflow-hidden border aspect-square transition-all ${
                        currentDisplayItem?.batchIndex === item.batchIndex && currentDisplayItem?.jobId === item.jobId
                          ? "border-brand-500 ring-2 ring-brand-500/30"
                          : "border-white/10 hover:border-white/20 opacity-70 hover:opacity-100"
                      }`}
                    >
                      {/* eslint-disable-next-line @next/next/no-img-element */}
                      <img src={item.imageUrl} alt={`seed ${item.seed}`} className="w-full h-full object-cover" />
                      <div className="absolute bottom-0 inset-x-0 bg-gradient-to-t from-black/80 to-transparent p-1">
                        <span className="font-mono text-[8px] text-zinc-300">#{item.seed}</span>
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            )}
          </GlassCard>
        ) : (
          <GlassCard className="flex-1 flex flex-col items-center justify-center p-12 border-white/10 text-center space-y-3">
            <div className="w-12 h-12 rounded-2xl border border-brand-500/20 bg-brand-500/[0.08] flex items-center justify-center text-brand-400">
              <IconSparkles className="size-6" />
            </div>
            <h3 className="text-sm font-semibold text-zinc-200">Pronto para gerar</h3>
            <p className="text-xs text-zinc-400 max-w-sm">
              Configure os parâmetros à esquerda e clique &ldquo;Gerar&rdquo; ou pressione{" "}
              <Kbd className="mx-0.5">Ctrl+Enter</Kbd>.
            </p>
          </GlassCard>
        )}

        {/* Histórico de sessão */}
        {history.length > 0 && (
          <div className="mt-4 shrink-0">
            <div className="flex items-center justify-between mb-2">
              <span className="font-mono text-[10px] text-zinc-400 uppercase tracking-wider">
                Sessão ({history.length})
              </span>
            </div>
            <div className="flex gap-2 overflow-x-auto pb-1">
              {history.slice(0, 20).map((item, idx) => {
                const isSelected =
                  currentDisplayItem?.jobId === item.jobId &&
                  currentDisplayItem?.batchIndex === item.batchIndex;
                return (
                  <button
                    key={`${item.jobId}-${item.batchIndex ?? idx}`}
                    type="button"
                    onClick={() => setCurrentDisplayItem(item)}
                    className={`shrink-0 size-14 rounded-lg overflow-hidden border transition-all ${
                      isSelected
                        ? "border-brand-500 ring-2 ring-brand-500/30"
                        : "border-white/10 opacity-60 hover:opacity-100"
                    }`}
                  >
                    {/* eslint-disable-next-line @next/next/no-img-element */}
                    <img src={item.thumbUrl || item.imageUrl} alt="" className="w-full h-full object-cover" />
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>

      {/* ═══════════════════════════════════════════════════════════════
          LIGHTBOX — Modal canônico glass-modal (Nível 3)
          ═══════════════════════════════════════════════════════════════ */}
      <Modal
        open={lightboxOpen && !!currentDisplayItem}
        onClose={() => setLightboxOpen(false)}
        title="Visualização"
        maxWidth="xl"
        bodyClassName="flex flex-col items-center gap-4"
      >
        {currentDisplayItem && (
          <>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={currentDisplayItem.imageUrl}
              alt={currentDisplayItem.prompt}
              className="max-h-[60vh] w-auto max-w-full object-contain rounded-xl border border-white/10"
            />
            <div className="w-full flex items-center justify-between gap-4 px-1">
              <p className="text-xs text-zinc-300 truncate max-w-md italic">
                &ldquo;{currentDisplayItem.prompt}&rdquo;
              </p>
              <div className="flex items-center gap-2 shrink-0">
                <Button size="sm" variant="primary" onClick={handleDownload}>
                  <IconDownload className="size-3.5 mr-1" />
                  Baixar
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setLightboxOpen(false)}>
                  Fechar
                </Button>
              </div>
            </div>
          </>
        )}
      </Modal>
    </div>
  );
}
