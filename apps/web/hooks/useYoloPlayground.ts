"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { getDataset, listDatasets } from "@/lib/datasets";
import { listImages } from "@/lib/images";
import { listJobs } from "@/lib/jobs";
import { listModels } from "@/lib/models";
import { getPredictions, startPredictJob } from "@/lib/playground";
import type { Dataset, Job, Model, PredictionsData } from "@/types/studio";
import { predictErrorMessage } from "@/types/studio";

export function useYoloPlayground() {
  const router = useRouter();

  /* ── Data ── */
  const [models, setModels] = useState<Model[]>([]);
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);

  /* ── Selections ── */
  const [selectedModelId, setSelectedModelId] = useState("");
  const [selectedDatasetId, setSelectedDatasetId] = useState("");
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<
    string | null
  >(null);
  const [conf, setConf] = useState(0.65);

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
  const pollingRef = useRef<number | null>(null);
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

  /* ── Fetch datasets (YOLO eligible only) ── */
  useEffect(() => {
    let active = true;
    listDatasets()
      .then((all) => {
        if (active) {
          setDatasets(
            all.filter((ds) => ds.category === "yolo" && ds.imagesCount > 0),
          );
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

  /* ── Fetch predict jobs ── */
  const fetchJobs = useCallback(async () => {
    try {
      const res = await listJobs();
      if (!activeRef.current) return;
      const predictJobs = res.items.filter(
        (j) => j.kind === "yolo_predict" || j.mode === "predict",
      );
      setJobs(predictJobs);
    } catch {
      // Ignora erro transitório
    }
  }, []);

  useEffect(() => {
    activeRef.current = true;
    void fetchJobs();

    const poll = () => {
      if (document.visibilityState === "visible") {
        void fetchJobs();
      }
    };
    pollingRef.current = window.setInterval(poll, 4000);

    const handleVis = () => {
      if (document.visibilityState === "visible") {
        void fetchJobs();
        if (pollingRef.current === null) {
          pollingRef.current = window.setInterval(poll, 4000);
        }
      } else if (pollingRef.current !== null) {
        window.clearInterval(pollingRef.current);
        pollingRef.current = null;
      }
    };

    document.addEventListener("visibilitychange", handleVis);
    return () => {
      activeRef.current = false;
      if (pollingRef.current !== null) {
        window.clearInterval(pollingRef.current);
        pollingRef.current = null;
      }
      document.removeEventListener("visibilitychange", handleVis);
    };
  }, [fetchJobs]);

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
        const preds = await getPredictions(jobId);
        if (loadRequestRef.current !== jobId) return;
        setPredictions(preds);

        const job = jobs.find((j) => j.id === jobId);
        if (job?.datasetId) {
          if (!datasetClassesCache.current[job.datasetId]) {
            try {
              const ds = await getDataset(job.datasetId);
              datasetClassesCache.current[job.datasetId] = ds.classes;
            } catch {
              // fallback neutro
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
              if (loadRequestRef.current !== jobId) return;
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
            // Imagens indisponíveis
          }
        }
      } catch (err) {
        const msg =
          err instanceof Error
            ? err.message
            : "Falha ao carregar resultados.";
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
          void loadJobResults(selectedJobId);
        }
      } else {
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
      showToast(
        res.status === "preparing"
          ? "Inferência aceita — preparando pacote. Acompanhe nas Execuções."
          : "Inferência iniciada — acompanhe nas Execuções.",
        "success",
        {
          label: "Ver Execuções",
          onClick: () => router.push(`/jobs?job=${res.jobId}`),
        },
      );
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

  const doneJobs = useMemo(
    () => jobs.filter((j) => j.status === "done"),
    [jobs],
  );
  const failedJobs = useMemo(
    () => jobs.filter((j) => j.status === "failed"),
    [jobs],
  );
  const activeJobs = useMemo(
    () =>
      jobs.filter(
        (j) =>
          j.status === "preparing" ||
          j.status === "queued" ||
          j.status === "dispatched" ||
          j.status === "running" ||
          j.status === "cancelling",
      ),
    [jobs],
  );

  useEffect(() => {
    if (doneJobs.length > 0 && !selectedJobId) {
      setSelectedJobId(doneJobs[0].id);
    }
  }, [doneJobs, selectedJobId]);

  const overlayStats = useMemo(
    () =>
      predictions
        ? {
            total: predictions.images.length,
            withDetections: predictions.images.filter(
              (i) => i.boxes.length > 0,
            ).length,
            totalBoxes: predictions.images.reduce(
              (sum, i) => sum + i.boxes.length,
              0,
            ),
            skips: predictions.images.filter(
              (i) => i.boxes.length > 0 && !imagesMap[i.filename],
            ).length,
          }
        : null,
    [predictions, imagesMap],
  );

  return {
    models,
    datasets,
    jobs,
    doneJobs,
    failedJobs,
    activeJobs,
    selectedModelId,
    setSelectedModelId,
    selectedDatasetId,
    setSelectedDatasetId,
    selectedOrchestratorId,
    setSelectedOrchestratorId,
    conf,
    setConf,
    submitting,
    selectedJobId,
    setSelectedJobId,
    predictions,
    imagesMap,
    loadingPredictions,
    loadingModels,
    loadingDatasets,
    modelsError,
    datasetsError,
    overlayStats,
    datasetClassesCache,
    handleSubmit,
    handleDownloadPredictions,
    loadJobResults,
  };
}
