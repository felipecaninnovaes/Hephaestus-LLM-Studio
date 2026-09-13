"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconDownload,
  IconPlay,
  IconRefresh,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import {
  Button,
  EmptyState,
  GlassCard,
  Select,
  SubmodulePills,
  type SelectOption,
  type SubmodulePillItem,
  showToast,
} from "@/components/ui";
import { listDatasets, getDataset } from "@/lib/datasets";
import { listJobs } from "@/lib/jobs";
import { listModels } from "@/lib/models";
import { getPredictions, startPredictJob } from "@/lib/playground";
import type {
  Dataset,
  Job,
  Model,
  PredictionsData,
} from "@/types/studio";
import { predictErrorMessage } from "@/types/studio";
import { ApiError } from "@/lib/api";
import { listImages } from "@/lib/images";
import NodeSelect from "@/components/studio/NodeSelect";
import PlaygroundDiffusion from "@/components/studio/PlaygroundDiffusion";

/* ── Cor da box no overlay: vem do dataset.classes, fallback neutro ── */
function classColor(
  cls: string,
  classes?: { name: string; color: string }[],
): string {
  return classes?.find((c) => c.name === cls)?.color ?? "#71717a";
}

/* ── Helpers ── */

function canPredict(ds: Dataset): boolean {
  return ds.category === "yolo" && ds.imagesCount > 0;
}

function modelLabel(m: Model): string {
  const parts = [m.name];
  if (m.source) parts.push(m.source);
  if (m.model) parts.push(m.model);
  return parts.join(" · ");
}

function datasetLabel(ds: Dataset): string {
  return `${ds.title} (${ds.imagesCount} imgs)`;
}

/* ═══════════════════════════════════════════════════════════════════
   Página /playground — Inferência YOLO real (Fatia J.5 / ADR-0013 D7)
   Workspace 2 colunas: esquerda = form, direita = resultados + overlay
   ═══════════════════════════════════════════════════════════════════ */

export default function PlaygroundPage() {
  const router = useRouter();

  /* ── Modo do Playground (Difusão vs YOLO) ── */
  const [playgroundMode, setPlaygroundMode] = useState<"diffusion" | "yolo">("diffusion");

  const modePills = useMemo<SubmodulePillItem<"diffusion" | "yolo">[]>(
    () => [
      {
        id: "diffusion",
        label: "Geração (Difusão)",
        icon: <IconSparkles className="size-3.5 text-brand-400" />,
      },
      {
        id: "yolo",
        label: "Detecção (YOLO)",
        icon: <IconTarget className="size-3.5 text-zinc-400" />,
      },
    ],
    [],
  );

  /* ── Data ── */
  const [models, setModels] = useState<Model[]>([]);
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);

  /* ── Selections ── */
  const [selectedModelId, setSelectedModelId] = useState("");
  const [selectedDatasetId, setSelectedDatasetId] = useState("");
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);
  const [conf, setConf] = useState(0.65);

  const modelOptions = useMemo<SelectOption<string>[]>(() => {
    return models.map((m) => ({
      value: m.id,
      label: modelLabel(m),
      description: m.source === "train" ? "Modelo treinado no estúdio" : undefined,
    }));
  }, [models]);

  const datasetOptions = useMemo<SelectOption<string>[]>(() => {
    return datasets.map((ds) => ({
      value: ds.id,
      label: datasetLabel(ds),
      description: `${ds.imagesCount} imagens · ${ds.classes?.length ?? 0} classes`,
    }));
  }, [datasets]);

  /* ── Submit state ── */
  const [submitting, setSubmitting] = useState(false);

  /* ── Results state ── */
  const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
  const [predictions, setPredictions] = useState<PredictionsData | null>(null);
  const [imagesMap, setImagesMap] = useState<
    Record<string, { url: string; width: number; height: number }>
  >({});
  const [loadingPredictions, setLoadingPredictions] = useState(false);

  /* ── Loading states ── */
  const [loadingModels, setLoadingModels] = useState(true);
  const [loadingDatasets, setLoadingDatasets] = useState(true);
  const [modelsError, setModelsError] = useState(false);
  const [datasetsError, setDatasetsError] = useState(false);

  /* ── Refs ── */
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const activeRef = useRef(true);
  const lastLoadedRef = useRef<string | null>(null);
  const loadRequestRef = useRef<string | null>(null);
  const datasetClassesCache = useRef<
    Record<string, { name: string; color: string }[]>
  >({});

  /* ── Fetch models (YOLO only) ── */
  useEffect(() => {
    let active = true;
    listModels()
      .then((res) => {
        if (active) {
          setModels(res.items.filter((m) => m.engine === "yolo"));
          setLoadingModels(false);
        }
      })
      .catch(() => {
        if (active) {
          setModelsError(true);
          setLoadingModels(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  /* ── Fetch datasets (YOLO with images) ── */
  useEffect(() => {
    let active = true;
    listDatasets()
      .then((data) => {
        if (active) {
          setDatasets(data.filter(canPredict));
          setLoadingDatasets(false);
        }
      })
      .catch(() => {
        if (active) {
          setDatasetsError(true);
          setLoadingDatasets(false);
        }
      });
    return () => {
      active = false;
    };
  }, []);

  /* ── Poll predict jobs (mode=predict) ── */
  useEffect(() => {
    activeRef.current = true;
    const fetchJobs = async () => {
      if (
        typeof document !== "undefined" &&
        document.visibilityState === "hidden"
      )
        return;
      try {
        const res = await listJobs();
        if (activeRef.current) {
          setJobs(
            res.items.filter(
              (j) =>
                j.mode === "predict" &&
                (j.status === "done" ||
                  j.status === "failed" ||
                  j.status === "running" ||
                  j.status === "queued" ||
                  j.status === "cancelling"),
            ),
          );
        }
      } catch {
        /* ignore */
      }
    };
    fetchJobs();
    pollingRef.current = setInterval(fetchJobs, 3000);
    const handleVis = () => {
      if (document.visibilityState === "visible") fetchJobs();
    };
    document.addEventListener("visibilitychange", handleVis);
    return () => {
      activeRef.current = false;
      if (pollingRef.current) clearInterval(pollingRef.current);
      document.removeEventListener("visibilitychange", handleVis);
    };
  }, []);

  /* ── Load predictions + images for selected job ── */
  const loadJobResults = useCallback(
    async (jobId: string) => {
      setSelectedJobId(jobId);
      setPredictions(null);
      setImagesMap({});
      setLoadingPredictions(true);
      lastLoadedRef.current = jobId;
      loadRequestRef.current = jobId;

      try {
        // 1. Carrega predictions.json do artefato
        const preds = await getPredictions(jobId);
        if (loadRequestRef.current !== jobId) return; // race: job mudou
        setPredictions(preds);

        // 2. Carrega imagens do dataset (via job vinculado), com paginação
        const job = jobs.find((j) => j.id === jobId);
        if (job?.datasetId) {
          // Cache de classes do dataset (B1)
          if (!datasetClassesCache.current[job.datasetId]) {
            try {
              const ds = await getDataset(job.datasetId);
              datasetClassesCache.current[job.datasetId] = ds.classes;
            } catch {
              /* dataset indisponível — fallback neutro */
            }
          }

          try {
            const map: Record<
              string,
              { url: string; width: number; height: number }
            > = {};
            let offset = 0;
            const limit = 200;
            let total = Infinity;
            while (offset < total) {
              const imgPage = await listImages(job.datasetId, {
                limit,
                offset,
              });
              if (loadRequestRef.current !== jobId) return; // race
              total = imgPage.total;
              for (const img of imgPage.items) {
                map[img.filename] = {
                  url: img.url,
                  width: img.width,
                  height: img.height,
                };
              }
              offset += imgPage.items.length;
              if (imgPage.items.length === 0) break;
            }
            setImagesMap(map);
          } catch {
            /* imagens indisponíveis — overlay sem imagens, honesto */
          }
        }
      } catch (err) {
        const msg =
          err instanceof Error ? err.message : "Falha ao carregar resultados.";
        showToast(msg, "error");
      } finally {
        if (loadRequestRef.current === jobId) {
          setLoadingPredictions(false);
        }
      }
    },
    [jobs],
  );

  /* ── Auto-load results ao selecionar job done ── */
  useEffect(() => {
    if (selectedJobId) {
      const job = jobs.find((j) => j.id === selectedJobId);
      if (job?.status === "done") {
        if (lastLoadedRef.current !== selectedJobId) {
          loadJobResults(selectedJobId);
        }
      } else {
        // Job saiu de done (ex: recém-criado, reprocessando) — reset ref
        lastLoadedRef.current = null;
      }
    }
  }, [selectedJobId, jobs, loadJobResults]);

  /* ── Submit predict job ── */
  async function handleSubmit() {
    if (!selectedModelId || !selectedDatasetId) return;
    setSubmitting(true);
    try {
      const res = await startPredictJob({
        modelId: selectedModelId,
        datasetId: selectedDatasetId,
        conf,
        orchestratorId: selectedOrchestratorId || undefined,
      });
      showToast("Inferência iniciada — acompanhe nas Execuções.", "success", {
        label: "Ver Execuções",
        onClick: () => router.push(`/jobs?job=${res.jobId}`),
      });
    } catch (err) {
      if (err instanceof ApiError) {
        showToast(predictErrorMessage(err.code), "error");
      } else {
        showToast("Falha ao iniciar inferência.", "error");
      }
    } finally {
      setSubmitting(false);
    }
  }

  /* ── Download predictions.json ── */
  async function handleDownloadPredictions(jobId: string) {
    try {
      const res = await getPredictions(jobId);
      const blob = new Blob([JSON.stringify(res, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      try {
        const a = document.createElement("a");
        a.href = url;
        a.download = "predictions.json";
        document.body.appendChild(a);
        a.click();
        a.remove();
      } finally {
        setTimeout(() => URL.revokeObjectURL(url), 1000);
      }
    } catch {
      showToast("Falha ao baixar predictions.json.", "error");
    }
  }

  /* ── Deriva jobs done/failed para exibir ── */
  const doneJobs = jobs.filter((j) => j.status === "done");
  const failedJobs = jobs.filter((j) => j.status === "failed");
  const activeJobs = jobs.filter(
    (j) => j.status === "queued" || j.status === "running" || j.status === "cancelling",
  );

  const modelsOnly = !loadingModels && models.length === 0;
  const datasetsOnly = !loadingDatasets && datasets.length === 0;

  /* ── Seleção automática: se há job done e nenhum selecionado, seleciona o mais recente ── */
  useEffect(() => {
    if (doneJobs.length > 0 && !selectedJobId) {
      setSelectedJobId(doneJobs[0].id);
    }
  }, [doneJobs, selectedJobId]);

  /* ── Stats do overlay ── */
  const overlayStats = predictions
    ? {
        total: predictions.images.length,
        withDetections: predictions.images.filter((i) => i.boxes.length > 0)
          .length,
        totalBoxes: predictions.images.reduce(
          (sum, i) => sum + i.boxes.length,
          0,
        ),
        skips: predictions.images.filter(
          (i) => i.boxes.length > 0 && !imagesMap[i.filename],
        ).length,
      }
    : null;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {/* ── Topbar com título e tabs de modo ── */}
      <div className="shrink-0 border-b border-white/5 bg-zinc-950/40 px-4 py-3 md:px-6 backdrop-blur-md flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center space-x-2.5">
          <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
            <IconPlay className="size-4" />
          </span>
          <div>
            <h1 className="font-display text-base md:text-lg font-bold text-white tracking-tight leading-none">
              Playground
            </h1>
            <p className="text-[11px] text-zinc-400 mt-0.5 font-mono">
              Ambiente interativo de experimentação e inferência rápida
            </p>
          </div>
        </div>

        <SubmodulePills<"diffusion" | "yolo">
          items={modePills}
          value={playgroundMode}
          onChange={setPlaygroundMode}
          size="sm"
        />
      </div>

      {/* ── Conteúdo selecionado ── */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {playgroundMode === "diffusion" ? (
          <div className="h-full overflow-y-auto p-4 md:p-6">
            <PlaygroundDiffusion />
          </div>
        ) : (
          <div className="flex h-full min-h-0 flex-col lg:flex-row">
            {/* ═══════════════════════════════════════════════════════════════
                COLUNA ESQUERDA — CONTROLES (320–384px)
                ═══════════════════════════════════════════════════════════════ */}
            <div className="w-full shrink-0 border-b border-white/5 p-4 md:p-5 lg:w-96 lg:border-b-0 lg:border-r lg:border-white/5 lg:overflow-y-auto">
              {/* Header YOLO */}
              <div className="mb-5 flex items-center space-x-2.5 border-b border-white/10 pb-4">
                <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
                  <IconTarget className="size-4" />
                </span>
                <div>
                  <h2 className="font-display text-sm font-bold text-white tracking-tight">
                    Detecção de Objetos
                  </h2>
                  <p className="text-[10px] text-zinc-400 font-mono">
                    Inferência em lote em datasets
                  </p>
                </div>
              </div>

        {/* Erro de rede: modelos */}
        {modelsError && (
          <GlassCard className="mb-4">
            <EmptyState
              icon={<IconTarget className="size-8 text-red-400" />}
              title="Falha ao carregar modelos"
              description="Não foi possível carregar a lista de modelos."
              actionLabel="Tentar novamente"
              onAction={() => {
                setModelsError(false);
                setLoadingModels(true);
                listModels()
                  .then((res) => {
                    setModels(res.items.filter((m) => m.engine === "yolo"));
                    setLoadingModels(false);
                  })
                  .catch(() => {
                    setModelsError(true);
                    setLoadingModels(false);
                  });
              }}
            />
          </GlassCard>
        )}

        {/* Erro de rede: datasets */}
        {datasetsError && (
          <GlassCard className="mb-4">
            <EmptyState
              icon={<IconTarget className="size-8 text-red-400" />}
              title="Falha ao carregar datasets"
              description="Não foi possível carregar a lista de datasets."
              actionLabel="Tentar novamente"
              onAction={() => {
                setDatasetsError(false);
                setLoadingDatasets(true);
                listDatasets()
                  .then((data) => {
                    setDatasets(data.filter(canPredict));
                    setLoadingDatasets(false);
                  })
                  .catch(() => {
                    setDatasetsError(true);
                    setLoadingDatasets(false);
                  });
              }}
            />
          </GlassCard>
        )}

        {/* Empty state: sem modelos YOLO */}
        {!modelsError && modelsOnly && (
          <GlassCard className="mb-4">
            <EmptyState
              icon={<IconTarget className="size-8 text-brand-400" />}
              title="Nenhum modelo YOLO disponível"
              description="Nenhum modelo ainda — treine em Treino YOLO ou envie pesos em Modelos & Pesos."
              actionLabel="Abrir Modelos & Pesos"
              onAction={() => router.push("/models")}
            />
          </GlassCard>
        )}

        {/* Empty state: sem datasets YOLO com imagens */}
        {!datasetsError && datasetsOnly && models.length > 0 && (
          <GlassCard className="mb-4">
            <EmptyState
              icon={<IconTarget className="size-8 text-brand-400" />}
              title="Nenhum dataset YOLO com imagens"
              description="Crie ou importe um dataset YOLO com imagens para inferir."
              actionLabel="Abrir Datasets"
              onAction={() => router.push("/datasets")}
            />
          </GlassCard>
        )}

        {/* Formulário de inferência */}
        {(models.length > 0 || datasets.length > 0) && (
          <div className="space-y-4">
            {/* ══ Modelo ══ */}
            <Select
              id="playground-model"
              label="Modelo"
              options={modelOptions}
              value={selectedModelId}
              onChange={(val) => setSelectedModelId(val)}
              disabled={loadingModels || models.length === 0}
              placeholder={
                loadingModels
                  ? "Carregando modelos…"
                  : models.length === 0
                    ? "Nenhum modelo YOLO"
                    : "Selecione um modelo"
              }
              hint={
                models.length > 0
                  ? `${models.length} modelo${models.length !== 1 ? "s" : ""} YOLO${
                      models.some((m) => m.source === "train")
                        ? ` · ${models.filter((m) => m.source === "train").length} de treino`
                        : ""
                    }`
                  : undefined
              }
              searchable={models.length > 5}
              fontMono
            />

            {/* ══ Dataset ══ */}
            <Select
              id="playground-dataset"
              label="Dataset"
              options={datasetOptions}
              value={selectedDatasetId}
              onChange={(val) => setSelectedDatasetId(val)}
              disabled={loadingDatasets || datasets.length === 0}
              placeholder={
                loadingDatasets
                  ? "Carregando datasets…"
                  : datasets.length === 0
                    ? "Nenhum dataset YOLO"
                    : "Selecione um dataset"
              }
              hint={
                datasets.length > 0
                  ? `${datasets.length} dataset${datasets.length !== 1 ? "s" : ""} YOLO · ${datasets.reduce((s, d) => s + d.imagesCount, 0)} imagens`
                  : undefined
              }
              searchable={datasets.length > 5}
              fontMono
            />

            {/* ══ Confidence Threshold ══ */}
            <div>
              <div className="mb-1.5 flex items-center justify-between">
                <label
                  htmlFor="playground-conf"
                  className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400"
                >
                  Confidence
                </label>
                <span className="font-mono text-xs tabular-nums text-zinc-300">
                  {conf.toFixed(2)}
                </span>
              </div>
              <div className="flex items-center gap-3">
                <input
                  id="playground-conf"
                  type="range"
                  min={0.3}
                  max={0.95}
                  step={0.01}
                  value={conf}
                  onChange={(e) => setConf(parseFloat(e.target.value))}
                  className="h-2 flex-1 cursor-pointer appearance-none rounded-full bg-zinc-800 accent-brand-500
                    [&::-webkit-slider-thumb]:size-5 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:border-2 [&::-webkit-slider-thumb]:border-white [&::-webkit-slider-thumb]:bg-brand-500 [&::-webkit-slider-thumb]:shadow-md [&::-webkit-slider-thumb]:shadow-brand-500/30
                    [&::-moz-range-thumb]:size-5 [&::-moz-range-thumb]:appearance-none [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:border-2 [&::-moz-range-thumb]:border-white [&::-moz-range-thumb]:bg-brand-500"
                />
              </div>
              <div className="mt-1 flex justify-between font-mono text-[10px] text-zinc-600">
                <span>0.30</span>
                <span>0.95</span>
              </div>
            </div>

            {/* ══ Nó de Execução (ADR-0015 D2) ══ */}
            <NodeSelect
              value={selectedOrchestratorId}
              onChange={setSelectedOrchestratorId}
              disabled={submitting}
              size="sm"
            />

            {/* ══ CTA Único — One CTA Rule ══ */}
            <Button
              type="button"
              variant="primary"
              size="lg"
              className="w-full"
              disabled={
                !selectedModelId ||
                !selectedDatasetId ||
                submitting ||
                loadingModels ||
                loadingDatasets
              }
              onClick={handleSubmit}
            >
              {submitting ? (
                <>
                  <span className="size-4 animate-spin rounded-full border-2 border-white/30 border-t-white" />
                  Iniciando…
                </>
              ) : (
                <>
                  <IconPlay className="size-4" />
                  Executar Inferência
                </>
              )}
            </Button>

            {/* Info */}
            <p className="text-center text-[11px] text-zinc-500">
              Inferência assíncrona — resultado aparece nas Execuções e aqui
              ao lado.
            </p>
          </div>
        )}
      </div>

      {/* ═══════════════════════════════════════════════════════════════
          COLUNA DIREITA — RESULTADOS + OVERLAY
          ═══════════════════════════════════════════════════════════════ */}
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        {/* Cabeçalho dos resultados */}
        <div className="flex shrink-0 items-center justify-between border-b border-white/5 px-4 py-3 md:px-5">
          <div className="flex items-center space-x-2">
            <IconTarget className="size-4 text-brand-400" />
            <span className="font-display text-sm font-semibold text-zinc-200">
              Resultados
            </span>
            {doneJobs.length > 0 && (
              <span className="rounded-full border border-brand-500/30 bg-brand-500/15 px-1.5 py-0.5 font-mono text-[11px] text-brand-300">
                {doneJobs.length}
              </span>
            )}
          </div>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => {
              listJobs().then((res) => {
                setJobs(
                  res.items.filter(
                    (j) =>
                      j.mode === "predict" &&
                      (j.status === "done" ||
                        j.status === "failed" ||
                        j.status === "running" ||
                        j.status === "queued" ||
                        j.status === "cancelling"),
                  ),
                );
              });
            }}
          >
            <IconRefresh className="size-3.5" />
          </Button>
        </div>

        {/* Conteúdo dos resultados */}
        <div className="flex-1 overflow-y-auto p-4 md:p-5">
          {/* Jobs ativos */}
          {activeJobs.length > 0 && (
            <div className="mb-4">
              <h3 className="mb-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400">
                Em execução
              </h3>
              <div className="space-y-2">
                {activeJobs.map((job) => (
                  <GlassCard
                    key={job.id}
                    className="flex items-center justify-between p-3"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="relative flex h-2 w-2 shrink-0">
                          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-brand-400 opacity-75 motion-reduce:animate-none" />
                          <span className="relative inline-flex h-2 w-2 rounded-full bg-brand-400" />
                        </span>
                        <span className="truncate font-mono text-xs text-zinc-200">
                          {job.model || "predict"}
                        </span>
                        <span className="rounded border border-zinc-800 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-zinc-400">
                          {job.status}
                        </span>
                      </div>
                      {job.queuePosition != null && job.queuePosition > 0 && (
                        <p className="mt-0.5 pl-4 font-mono text-[11px] text-zinc-500">
                          Posição na fila: {job.queuePosition}
                        </p>
                      )}
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => router.push(`/jobs?job=${job.id}`)}
                    >
                      Ver
                    </Button>
                  </GlassCard>
                ))}
              </div>
            </div>
          )}

          {/* Jobs failed */}
          {failedJobs.length > 0 && (
            <div className="mb-4">
              <h3 className="mb-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-red-400">
                Falhou
              </h3>
              <div className="space-y-2">
                {failedJobs.map((job) => (
                  <GlassCard
                    key={job.id}
                    className="flex items-center justify-between p-3"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="relative flex h-2 w-2 shrink-0">
                          <span className="relative inline-flex h-2 w-2 rounded-full bg-red-400" />
                        </span>
                        <span className="truncate font-mono text-xs text-zinc-200">
                          {job.model || "predict"}
                        </span>
                        <span className="rounded border border-red-800/40 bg-red-900/30 px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider text-red-400">
                          failed
                        </span>
                      </div>
                      {job.queueReason && (
                        <p className="mt-0.5 pl-4 font-mono text-[11px] text-zinc-500 truncate">
                          {job.queueReason}
                        </p>
                      )}
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => router.push(`/jobs?job=${job.id}`)}
                    >
                      Ver em Execuções
                    </Button>
                  </GlassCard>
                ))}
              </div>
            </div>
          )}

          {/* Lista de jobs predict concluídos */}
          {doneJobs.length > 0 && (
            <div className="mb-4">
              <h3 className="mb-2 font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-400">
                Concluídos
              </h3>
              <div className="space-y-1.5">
                {doneJobs.map((job) => (
                  <div
                    key={job.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => setSelectedJobId(job.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        setSelectedJobId(job.id);
                      }
                    }}
                    className={`group flex w-full items-center justify-between rounded-lg border px-3 py-2 text-left transition-colors ${
                      selectedJobId === job.id
                        ? "border-brand-500/30 bg-brand-500/15"
                        : "border-transparent hover:bg-white/[0.04]"
                    } cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70`}
                  >
                    <div className="min-w-0 flex-1">
                      <span className="truncate font-mono text-xs text-zinc-200">
                        {job.model || "predict"}
                      </span>
                      <span className="ml-2 font-mono text-[10px] text-zinc-500">
                        {new Date(job.createdAt).toLocaleDateString("pt-BR", {
                          day: "2-digit",
                          month: "2-digit",
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </span>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="opacity-0 group-hover:opacity-100"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleDownloadPredictions(job.id);
                        }}
                        title="Baixar predictions.json"
                      >
                        <IconDownload className="size-3.5" />
                      </Button>
                      <span className="rounded-full bg-[#34d399]/10 px-1.5 py-0.5 font-mono text-[10px] text-[#34d399]">
                        done
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Loading predictions */}
          {loadingPredictions && (
            <div className="flex flex-col items-center justify-center py-16 text-center">
              <span className="mb-3 size-8 animate-spin rounded-full border-2 border-brand-500/30 border-t-brand-400" />
              <p className="font-mono text-xs text-zinc-400">
                Carregando predições…
              </p>
            </div>
          )}

          {/* Overlay de predições */}
          {predictions && !loadingPredictions && (
            <div>
              {/* Estatísticas */}
              {overlayStats && (
                <div className="mb-4 flex flex-wrap gap-3">
                  <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                    <span className="block font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                      Imagens
                    </span>
                    <span className="font-mono text-sm tabular-nums text-zinc-200">
                      {overlayStats.total}
                    </span>
                  </div>
                  <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                    <span className="block font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                      Com detecção
                    </span>
                    <span className="font-mono text-sm tabular-nums text-zinc-200">
                      {overlayStats.withDetections}
                    </span>
                  </div>
                  <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                    <span className="block font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                      Total boxes
                    </span>
                    <span className="font-mono text-sm tabular-nums text-brand-300">
                      {overlayStats.totalBoxes}
                    </span>
                  </div>
                  {overlayStats.skips > 0 && (
                    <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                      <span className="block font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                        Skips
                      </span>
                      <span className="font-mono text-sm tabular-nums text-amber-400">
                        {overlayStats.skips}
                      </span>
                    </div>
                  )}
                  <div className="rounded-lg border border-white/5 bg-white/[0.02] px-3 py-2">
                    <span className="block font-mono text-[10px] uppercase tracking-wider text-zinc-500">
                      Conf
                    </span>
                    <span className="font-mono text-sm tabular-nums text-zinc-300">
                      ≥ {predictions.conf.toFixed(2)}
                    </span>
                  </div>
                </div>
              )}

              {/* Download button */}
              {selectedJobId && (
                <div className="mb-4">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() => handleDownloadPredictions(selectedJobId)}
                  >
                    <IconDownload className="size-3.5" />
                    Baixar predictions.json
                  </Button>
                </div>
              )}

              {/* Grid de imagens com overlay */}
              <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
                {predictions.images.map((predImg) => {
                  const img = imagesMap[predImg.filename];
                  const hasBoxes = predImg.boxes.length > 0;

                  return (
                    <div
                      key={predImg.filename}
                      className="glass-card overflow-hidden rounded-xl"
                    >
                      {/* Imagem com overlay de boxes */}
                      <div className="relative bg-zinc-900/50">
                        {img ? (
                          /* eslint-disable-next-line @next/next/no-img-element */
                          <img
                            src={img.url}
                            alt={predImg.filename}
                            className="block w-full object-contain"
                            style={{
                              aspectRatio:
                                img.width && img.height
                                  ? `${img.width}/${img.height}`
                                  : undefined,
                            }}
                            loading="lazy"
                          />
                        ) : (
                          <div className="flex aspect-video flex-col items-center justify-center gap-1 text-zinc-600">
                            <span className="font-mono text-[11px]">
                              {predImg.filename}
                            </span>
                            <span className="font-mono text-[10px] text-zinc-500">
                              imagem não encontrada no dataset
                            </span>
                          </div>
                        )}

                        {/* Bounding boxes overlay */}
                        {hasBoxes &&
                          predImg.boxes.map((box, bi) => {
                            const job = jobs.find((j) => j.id === selectedJobId);
                            const classes = job?.datasetId
                              ? datasetClassesCache.current[job.datasetId]
                              : undefined;
                            const color = classColor(box.class, classes);
                            return (
                              <div
                                key={bi}
                                className="absolute"
                                style={{
                                  left: `${box.x * 100}%`,
                                  top: `${box.y * 100}%`,
                                  width: `${box.w * 100}%`,
                                  height: `${box.h * 100}%`,
                                  border: `1.5px solid ${color}`,
                                  boxShadow: `0 0 0 1px ${color}33`,
                                }}
                              >
                                {/* Badge de classe */}
                                <span
                                  className="absolute -top-2.5 left-0 flex items-center gap-1 rounded-sm px-1 py-px font-mono text-[9px] font-medium leading-tight"
                                  style={{
                                    backgroundColor: `${color}22`,
                                    color,
                                    border: `1px solid ${color}44`,
                                  }}
                                >
                                  {box.class}
                                  <span style={{ opacity: 0.7 }}>
                                    {box.conf.toFixed(2)}
                                  </span>
                                </span>
                              </div>
                            );
                          })}
                      </div>

                      {/* Rodapé do card */}
                      <div className="flex items-center justify-between px-3 py-2">
                        <span
                          className="min-w-0 truncate font-mono text-[11px] text-zinc-400"
                          title={predImg.filename}
                        >
                          {predImg.filename}
                        </span>
                        <span
                          className={`shrink-0 rounded-full px-1.5 py-0.5 font-mono text-[10px] ${
                            hasBoxes
                              ? "bg-brand-500/15 text-brand-300"
                              : "bg-white/5 text-zinc-500"
                          }`}
                        >
                          {predImg.boxes.length === 0
                            ? "sem detecção"
                            : `${predImg.boxes.length} box${predImg.boxes.length !== 1 ? "es" : ""}`}
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          {/* Empty state: nenhum job ainda */}
          {!loadingPredictions &&
            doneJobs.length === 0 &&
            activeJobs.length === 0 &&
            failedJobs.length === 0 &&
            !predictions && (
              <div className="flex flex-col items-center justify-center py-16 text-center">
                <EmptyState
                  icon={<IconTarget className="size-8 text-brand-400" />}
                  title="Nenhuma execução de playground ainda"
                  description="Selecione um modelo e um dataset à esquerda para iniciar uma inferência."
                />
              </div>
            )}
        </div>
      </div>
          </div>
        )}
      </div>
    </div>
  );
}
