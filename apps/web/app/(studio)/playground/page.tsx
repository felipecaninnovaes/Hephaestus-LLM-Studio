"use client";

import { IconTarget } from "@/components/icons";
import {
  PlaygroundConfigCard,
  PlaygroundResultsPanel,
} from "@/components/studio/playground";
import { useYoloPlayground } from "@/hooks/useYoloPlayground";

export default function PlaygroundPage() {
  const playground = useYoloPlayground();
  const {
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
  } = playground;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {/* ── Topbar com título ── */}
      <div className="shrink-0 border-b border-white/5 bg-zinc-950/40 px-4 py-3 md:px-6 backdrop-blur-md flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center space-x-2.5">
          <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
            <IconTarget className="size-4" />
          </span>
          <div>
            <h1 className="font-display text-base md:text-lg font-bold text-white tracking-tight leading-none">
              Detecção YOLO
            </h1>
            <p className="text-2xs text-zinc-400 mt-0.5 font-mono">
              Inferência em lote em datasets
            </p>
          </div>
        </div>
      </div>

      {/* ── Workspace 2 colunas: esquerda = form, direita = resultados ── */}
      <div className="flex h-full min-h-0 flex-col lg:flex-row">
        <PlaygroundConfigCard
          models={models}
          datasets={datasets}
          selectedModelId={selectedModelId}
          setSelectedModelId={setSelectedModelId}
          selectedDatasetId={selectedDatasetId}
          setSelectedDatasetId={setSelectedDatasetId}
          selectedOrchestratorId={selectedOrchestratorId}
          setSelectedOrchestratorId={setSelectedOrchestratorId}
          conf={conf}
          setConf={setConf}
          submitting={submitting}
          loadingModels={loadingModels}
          loadingDatasets={loadingDatasets}
          modelsError={modelsError}
          datasetsError={datasetsError}
          onRetryModels={() => window.location.reload()}
          onRetryDatasets={() => window.location.reload()}
          onSubmit={() => void handleSubmit()}
        />

        <PlaygroundResultsPanel
          doneJobs={doneJobs}
          activeJobs={activeJobs}
          failedJobs={failedJobs}
          selectedJobId={selectedJobId}
          onSelectJob={(id) => void loadJobResults(id)}
          predictions={predictions}
          imagesMap={imagesMap}
          loadingPredictions={loadingPredictions}
          overlayStats={overlayStats}
          datasetClassesCache={datasetClassesCache}
          jobs={jobs}
          onRefreshJobs={() => window.location.reload()}
          onDownloadPredictions={(id) => void handleDownloadPredictions(id)}
        />
      </div>
    </div>
  );
}
