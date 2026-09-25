"use client";

import Link from "next/link";
import { useState, useEffect, useMemo } from "react";
import {
  IconBoxSelect,
  IconLayers,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import { formatBytes, formatPercent, formatRelativeTime } from "@/lib/format";
import { listImages, getImage } from "@/lib/images";
import { canTrainDataset, trainDatasetDisabledReason, trainDatasetActionLabel } from "@/lib/datasets";
import { Badge } from "@/components/ui/Badge";
import {
  STATUS_LABELS,
  type Dataset,
  type ImageItem,
  type BBoxData,
  type StudioClass,
} from "@/types/studio";

const STATUS_BADGE_VARIANT: Record<Dataset["status"], "ready" | "alert" | "info"> = {
  needs_labeling: "alert",
  in_progress: "info",
  ready: "ready",
};

const CATEGORY_LABELS: Record<Dataset["category"], string> = {
  difusao: "Difusão",
  openclip: "OpenCLIP",
  yolo: "YOLO",
};

function CategoryIcon({ category }: { category: Dataset["category"] }) {
  if (category === "yolo")
    return <IconTarget className="w-4 h-4 text-zinc-300" />;
  if (category === "difusao")
    return <IconSparkles className="w-4 h-4 text-zinc-300" />;
  return <IconLayers className="w-4 h-4 text-zinc-300" />;
}


/** Vitrine Óptica de Visão Computacional (Mosaico simétrico de miniaturas + bounding boxes). */
function OpticalShowcase({ dataset }: { dataset: Dataset }) {
  const [images, setImages] = useState<ImageItem[]>([]);
  const [heroBoxes, setHeroBoxes] = useState<BBoxData[]>([]);
  const [loading, setLoading] = useState(dataset.imagesCount > 0);
  const [imageError, setImageError] = useState(false);

  const classMap = useMemo(() => {
    const map = new Map<string, StudioClass>();
    for (const c of dataset.classes) map.set(c.id, c);
    return map;
  }, [dataset.classes]);

  useEffect(() => {
    if (dataset.imagesCount === 0) {
      setLoading(false);
      return;
    }

    let active = true;
    async function fetchPreviews() {
      try {
        const res = await listImages(dataset.id, { limit: 3, deleted: false });
        if (!active) return;
        const items = res.items ?? [];
        setImages(items);

        if (items.length > 0 && dataset.labeledCount > 0) {
          try {
            const detail = await getImage(dataset.id, items[0].id);
            if (active && detail.boxes) {
              setHeroBoxes(detail.boxes);
            }
          } catch {
            // Detalhes de boxes opcionais, não bloqueiam a renderização
          }
        }
      } catch {
        if (active) setImageError(true);
      } finally {
        if (active) setLoading(false);
      }
    }

    fetchPreviews();
    return () => {
      active = false;
    };
  }, [dataset.id, dataset.imagesCount, dataset.labeledCount]);

  if (dataset.imagesCount === 0 || imageError || (!loading && images.length === 0)) {
    return (
      <div className="relative h-32 w-full overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-950/70 backdrop-blur-sm flex flex-col items-center justify-center gap-1.5 group-hover:border-zinc-700/60 transition-colors">
        <div className="absolute inset-0 bg-[radial-gradient(#3f3f46_1px,transparent_1px)] opacity-30 [background-size:12px_12px]" />
        
        {/* Retículas ópticas simétricas nos 4 vértices */}
        <span className="absolute top-1.5 left-2 font-mono text-2xs leading-none text-zinc-600 pointer-events-none select-none">+</span>
        <span className="absolute top-1.5 right-2 font-mono text-2xs leading-none text-zinc-600 pointer-events-none select-none">+</span>
        <span className="absolute bottom-1.5 left-2 font-mono text-2xs leading-none text-zinc-600 pointer-events-none select-none">+</span>
        <span className="absolute bottom-1.5 right-2 font-mono text-2xs leading-none text-zinc-600 pointer-events-none select-none">+</span>

        <IconBoxSelect className="w-5 h-5 text-zinc-600 group-hover:text-zinc-500 transition-colors relative z-10" />
        <span className="font-mono text-2xs tracking-caps uppercase text-zinc-400 relative z-10">
          Sem amostras visuais
        </span>
        <span className="font-mono text-2xs text-zinc-400 relative z-10">
          0 imagens · ingestão pendente
        </span>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="relative h-32 w-full overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-950/80 backdrop-blur-sm animate-pulse flex items-center justify-center">
        <span className="font-mono text-2xs text-zinc-400 tracking-caps uppercase">
          Carregando ótica…
        </span>
      </div>
    );
  }

  const isSingle = images.length === 1;
  const isDual = images.length === 2;
  const hero = images[0];

  return (
    <div className="relative h-32 w-full overflow-hidden rounded-xl border border-zinc-800/80 bg-zinc-950 group-hover:border-brand-500/40 transition-all flex">
      {/* Retículas zenitais de mira ótica */}
      <span className="absolute top-1.5 left-2 z-20 font-mono text-2xs leading-none text-white/60 pointer-events-none select-none drop-shadow-sm">+</span>
      <span className="absolute top-1.5 right-2 z-20 font-mono text-2xs leading-none text-white/60 pointer-events-none select-none drop-shadow-sm">+</span>

      {/* Amostra Principal / Hero com BBoxes */}
      <div
        className={`relative h-full overflow-hidden bg-zinc-950 ${
          isSingle ? "w-full" : isDual ? "w-1/2 border-r border-zinc-800/80" : "w-3/5 border-r border-zinc-800/80"
        }`}
      >
        <img
          src={hero.url}
          alt={hero.filename}
          loading="lazy"
          decoding="async"
          className="absolute inset-0 h-full w-full object-cover"
        />
        <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-black/20 pointer-events-none" />

        {/* Caixas delimitadoras com cores semânticas da classe */}
        {heroBoxes.map((box) => {
          const cls = classMap.get(box.classId);
          const color = cls?.color || "#34d399";
          const isNearTop = box.y < 0.12;

          return (
            <div
              key={box.id}
              className="absolute pointer-events-none border transition-all"
              style={{
                left: `${Math.max(0, Math.min(100, box.x * 100))}%`,
                top: `${Math.max(0, Math.min(100, box.y * 100))}%`,
                width: `${Math.max(3, Math.min(100, box.w * 100))}%`,
                height: `${Math.max(3, Math.min(100, box.h * 100))}%`,
                borderColor: color,
                backgroundColor: `${color}24`,
                boxShadow: `0 0 8px ${color}40`,
              }}
            >
              <span
                className={`absolute ${isNearTop ? "top-0.5 left-0.5" : "-top-4 left-0"} px-1 py-0.2 rounded-sm font-mono text-2xs font-bold text-zinc-950 uppercase tracking-tighter truncate max-w-[80px] shadow-sm`}
                style={{ backgroundColor: color }}
              >
                {cls?.name ?? "obj"}
              </span>
            </div>
          );
        })}

        {/* Rodapé técnico da miniatura principal */}
        <div className="absolute inset-x-0 bottom-0 px-1.5 py-1 flex items-center justify-between gap-1 z-10 pointer-events-none">
          <span
            className="font-mono text-2xs text-zinc-200 truncate min-w-0 px-1.5 py-0.2 rounded bg-black/80 border border-white/10 backdrop-blur-sm"
            title={hero.filename}
          >
            {hero.filename}
          </span>
          {hero.split && (
            <span className="shrink-0 font-mono text-2xs uppercase tracking-caps px-1.5 py-0.2 rounded bg-black/80 text-zinc-300 border border-white/15 backdrop-blur-sm">
              {hero.split}
            </span>
          )}
        </div>
      </div>

      {/* Painel Secundário Simétrico: 50% quando dual, ou 40% empilhado quando 3+ */}
      {isDual && (
        <div className="relative w-1/2 h-full overflow-hidden bg-zinc-950">
          <img
            src={images[1].url}
            alt={images[1].filename}
            loading="lazy"
            decoding="async"
            className="absolute inset-0 h-full w-full object-cover"
          />
          <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-black/20 pointer-events-none" />
          <div className="absolute inset-x-0 bottom-0 px-1.5 py-1 flex items-center justify-between gap-1 z-10 pointer-events-none">
            <span
              className="font-mono text-2xs text-zinc-200 truncate min-w-0 px-1.5 py-0.2 rounded bg-black/80 border border-white/10 backdrop-blur-sm"
              title={images[1].filename}
            >
              {images[1].filename}
            </span>
            {images[1].split && (
              <span className="shrink-0 font-mono text-2xs uppercase tracking-caps px-1.5 py-0.2 rounded bg-black/80 text-zinc-300 border border-white/15 backdrop-blur-sm">
                {images[1].split}
              </span>
            )}
          </div>
        </div>
      )}

      {!isSingle && !isDual && (
        <div className="w-2/5 h-full flex flex-col divide-y divide-zinc-800/80 bg-zinc-950/90">
          {images.slice(1, 3).map((img) => (
            <div key={img.id} className="relative flex-1 overflow-hidden group/sub">
              <img
                src={img.url}
                alt={img.filename}
                loading="lazy"
                decoding="async"
                className="absolute inset-0 h-full w-full object-cover opacity-80 group-hover/sub:opacity-100 transition-opacity"
              />
              <div className="absolute inset-0 bg-black/25 pointer-events-none" />
              {img.split && (
                <span className="absolute top-1 right-1 font-mono text-2xs uppercase tracking-caps px-1 rounded bg-black/80 text-zinc-300 border border-white/15">
                  {img.split}
                </span>
              )}
            </div>
          ))}
          {dataset.imagesCount > images.length && (
            <div className="absolute bottom-1.5 right-1.5 z-20 font-mono text-2xs px-1.5 py-0.5 rounded-md bg-black/85 text-zinc-300 border border-white/20 backdrop-blur-sm shadow-md">
              +{dataset.imagesCount - images.length}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default function DatasetCard({
  dataset,
  onContextMenu,
  onTrain,
}: {
  dataset: Dataset;
  onContextMenu?: (dataset: Dataset, x: number, y: number) => void;
  onTrain?: (dataset: Dataset) => void;
}) {
  const visibleClasses = dataset.classes.slice(0, 4);
  const extra = dataset.classes.length - visibleClasses.length;
  const categoryLabel = CATEGORY_LABELS[dataset.category] ?? dataset.category;
  const trainEnabled = canTrainDataset(dataset);
  const trainTitle = trainEnabled ? trainDatasetActionLabel(dataset) : trainDatasetDisabledReason(dataset);

  return (
    <div
      className="glass-card relative group rounded-2xl p-5 flex flex-col h-full transition-all hover:border-brand-500/30"
    >
      {/* Stretch link para navegação do card sem aninhamento de botões */}
      <Link
        href={`/datasets/${dataset.id}`}
        className="absolute inset-0 z-0 rounded-2xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
        aria-label={`Abrir dataset ${dataset.title}`}
        onContextMenu={
          onContextMenu
            ? (e) => {
                e.preventDefault();
                onContextMenu(dataset, e.clientX, e.clientY);
              }
            : undefined
        }
      />

      {/* 1. Cabeçalho de Categoria e Status (Altura padronizada h-8) */}
      <div className="relative z-10 pointer-events-none flex items-center justify-between gap-2 h-8">
        <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-zinc-900/90 border border-zinc-700/80 backdrop-blur-sm text-zinc-300 group-hover:text-brand-400 transition-colors">
          <CategoryIcon category={dataset.category} />
        </span>
        <span className="flex min-w-0 items-center justify-end gap-1.5 pointer-events-auto">
          {dataset.autoTracked && (
            <Badge variant="telemetry" title="AutoTracker">
              AutoTracker
            </Badge>
          )}
          <Badge
            variant={STATUS_BADGE_VARIANT[dataset.status]}
            title={STATUS_LABELS[dataset.status]}
          >
            {STATUS_LABELS[dataset.status]}
          </Badge>
        </span>
      </div>

      {/* 2. Identificação Textual (Altura padronizada h-[58px] para alinhamento baseline) */}
      <div className="relative z-10 pointer-events-none min-w-0 mt-3 h-[58px] flex flex-col justify-start">
        <p className="truncate text-sm font-semibold text-zinc-100 group-hover:text-white transition-colors" title={dataset.title}>
          {dataset.title}
        </p>
        <p className="truncate font-mono text-2xs text-zinc-400 mt-0.5" title={dataset.slug}>
          {dataset.slug}
        </p>
        <p className="tracking-caps mt-1 truncate font-mono text-2xs uppercase text-zinc-400" title={categoryLabel}>
          {categoryLabel}
        </p>
      </div>

      {/* 3. Mosaico Óptico (Sempre inicia exatamente na mesma coordenada Y) */}
      <div className="relative z-10 pointer-events-none mt-3.5">
        <OpticalShowcase dataset={dataset} />
      </div>

      {/* 4. Métricas Principais (Sempre alinhadas na mesma linha horizontal) */}
      <div className="relative z-10 pointer-events-none grid grid-cols-2 gap-2 mt-3.5">
        <div className="p-2.5 rounded-xl bg-zinc-900/80 border border-zinc-800/60 backdrop-blur-sm">
          <p className="tracking-caps font-mono text-2xs uppercase text-zinc-400">
            Imagens
          </p>
          <p className="font-mono text-sm font-bold text-zinc-200 mt-0.5">
            {dataset.imagesCount.toLocaleString()}
          </p>
        </div>
        <div className="p-2.5 rounded-xl bg-zinc-900/80 border border-zinc-800/60 backdrop-blur-sm">
          <p className="tracking-caps font-mono text-2xs uppercase text-zinc-400">
            Rotuladas
          </p>
          <p className="font-mono text-sm font-bold text-status-success mt-0.5">
            {formatPercent(dataset.labeledCount, dataset.imagesCount)}
          </p>
        </div>
      </div>

      {/* 5. Seção de Classes (Altura consistente min-h-[64px] garantindo simetria) */}
      <div className="relative z-10 pointer-events-none mt-3.5 min-h-[64px] flex flex-col justify-start">
        <div className="flex items-center justify-between mb-1">
          <span className="text-2xs text-zinc-400 font-mono uppercase tracking-caps">
            Classes ({dataset.classes.length}):
          </span>
        </div>

        {dataset.classes.length > 0 ? (
          <>
            {/* Faixa óptica de distribuição cromática */}
            <div className="h-1 w-full rounded-full overflow-hidden flex gap-0.5 bg-zinc-900 border border-zinc-800/60 mb-2">
              {dataset.classes.map((c) => (
                <div
                  key={c.id}
                  className="h-full flex-1 transition-all"
                  style={{ backgroundColor: c.color }}
                  title={`${c.name} (${c.color})`}
                />
              ))}
            </div>

            {/* Chips de classes com dot cromático */}
            <div className="flex flex-wrap gap-1.5 pointer-events-auto">
              {visibleClasses.map((c) => (
                <span
                  key={c.id}
                  title={`${c.name} (${c.color})`}
                  className="inline-flex items-center gap-1.5 text-2xs font-mono px-2 py-0.5 rounded-md bg-zinc-900/90 backdrop-blur-sm text-zinc-300 border border-zinc-800 max-w-full truncate group-hover:border-zinc-700/80 transition-colors"
                >
                  <span
                    className="w-2 h-2 rounded-full shrink-0 shadow-sm"
                    style={{ backgroundColor: c.color }}
                    aria-hidden="true"
                  />
                  <span className="truncate">{c.name}</span>
                </span>
              ))}
              {extra > 0 && (
                <span
                  title={`Mais ${extra} classes`}
                  className="inline-flex items-center text-2xs font-mono px-2 py-0.5 rounded-md bg-zinc-900/80 backdrop-blur-sm text-zinc-400 border border-zinc-800"
                >
                  +{extra}
                </span>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="h-1 w-full rounded-full bg-zinc-900/60 border border-zinc-800/40 mb-2 opacity-50" />
            <div className="flex items-center">
              <span className="inline-flex items-center gap-1.5 text-2xs font-mono px-2 py-0.5 rounded-md bg-zinc-900/50 backdrop-blur-sm text-zinc-400 border border-dashed border-zinc-800/80">
                Nenhuma classe cadastrada
              </span>
            </div>
          </>
        )}
      </div>

      {/* 6. Rodapé (Ancorado no final com mt-auto, alinhando a linha de base em 100% dos cards) */}
      <div className="relative z-10 mt-auto pt-3 border-t border-zinc-800/80 flex items-center justify-between gap-2 text-2xs text-zinc-400 font-mono">
        <span className="min-w-0 truncate" title={`${formatBytes(dataset.sizeBytes)} · ${formatRelativeTime(dataset.lastModified)}`}>
          {formatBytes(dataset.sizeBytes)} · {formatRelativeTime(dataset.lastModified)}
        </span>
        <button
          type="button"
          disabled={!trainEnabled}
          title={trainTitle}
          onClick={(e) => {
            e.stopPropagation();
            if (trainEnabled && onTrain) onTrain(dataset);
          }}
          className={`shrink-0 font-medium cursor-pointer rounded px-1.5 py-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
            trainEnabled
              ? "text-brand-400 transition-colors hover:text-brand-300"
              : "cursor-not-allowed text-brand-400/50"
          }`}
        >
          Treinar →
        </button>
      </div>
    </div>
  );
}
