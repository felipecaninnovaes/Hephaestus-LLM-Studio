"use client";

import { useRouter } from "next/navigation";
import { IconPlay, IconTarget } from "@/components/icons";
import NodeSelect from "@/components/studio/NodeSelect";
import { Button } from "@/components/ui/Button";
import { EmptyState } from "@/components/ui/EmptyState";
import { GlassCard } from "@/components/ui/GlassCard";
import { Select, type SelectOption } from "@/components/ui/Select";
import type { Dataset, Model } from "@/types/studio";

export interface PlaygroundConfigCardProps {
  models: Model[];
  datasets: Dataset[];
  selectedModelId: string;
  setSelectedModelId: (id: string) => void;
  selectedDatasetId: string;
  setSelectedDatasetId: (id: string) => void;
  selectedOrchestratorId: string | null;
  setSelectedOrchestratorId: (id: string | null) => void;
  conf: number;
  setConf: (val: number) => void;
  submitting: boolean;
  loadingModels: boolean;
  loadingDatasets: boolean;
  modelsError: boolean;
  datasetsError: boolean;
  onRetryModels: () => void;
  onRetryDatasets: () => void;
  onSubmit: () => void;
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

export function PlaygroundConfigCard({
  models,
  datasets,
  selectedModelId,
  setSelectedModelId,
  selectedDatasetId,
  setSelectedDatasetId,
  selectedOrchestratorId,
  setSelectedOrchestratorId,
  conf,
  setConf,
  submitting,
  loadingModels,
  loadingDatasets,
  modelsError,
  datasetsError,
  onRetryModels,
  onRetryDatasets,
  onSubmit,
}: PlaygroundConfigCardProps) {
  const router = useRouter();

  const modelOptions: SelectOption<string>[] = models.map((m) => ({
    value: m.id,
    label: modelLabel(m),
    description:
      m.source === "train" ? "Modelo treinado no estúdio" : undefined,
  }));

  const datasetOptions: SelectOption<string>[] = datasets.map((ds) => ({
    value: ds.id,
    label: datasetLabel(ds),
    description: `${ds.imagesCount} imagens · ${ds.classes?.length ?? 0} classes`,
  }));

  const modelsOnly = !loadingModels && models.length === 0;
  const datasetsOnly = !loadingDatasets && datasets.length === 0;

  return (
    <div className="w-full shrink-0 border-b border-white/5 p-4 md:p-5 lg:w-96 lg:border-b-0 lg:border-r lg:border-white/5 lg:overflow-y-auto">
      {/* Erro de rede: modelos */}
      {modelsError && (
        <GlassCard className="mb-4">
          <EmptyState
            icon={<IconTarget className="size-8 text-red-400" />}
            title="Falha ao carregar modelos"
            description="Não foi possível carregar a lista de modelos."
            actionLabel="Tentar novamente"
            onAction={onRetryModels}
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
            onAction={onRetryDatasets}
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
                className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400"
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
            <div className="mt-1 flex justify-between font-mono text-3xs text-zinc-600">
              <span>0.30</span>
              <span>0.95</span>
            </div>
          </div>

          {/* ══ Nó de Execução ══ */}
          <NodeSelect
            value={selectedOrchestratorId}
            onChange={setSelectedOrchestratorId}
            disabled={submitting}
            size="sm"
          />

          {/* ══ CTA Único ══ */}
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
            loading={submitting}
            onClick={onSubmit}
          >
            <IconPlay className="size-4" />
            {submitting ? "Iniciando…" : "Executar Detecção"}
          </Button>
        </div>
      )}
    </div>
  );
}
