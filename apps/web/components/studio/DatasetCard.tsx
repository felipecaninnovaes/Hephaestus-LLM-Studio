import Link from "next/link";
import {
  IconLayers,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import { formatBytes, formatPercent, formatRelativeTime } from "@/lib/format";
import { STATUS_LABELS, type Dataset } from "@/types/studio";

const STATUS_STYLES: Record<Dataset["status"], string> = {
  needs_labeling: "text-amber-300 border-amber-400/30 bg-amber-400/10",
  in_progress: "text-cyan-300 border-cyan-400/30 bg-cyan-400/10",
  ready: "text-[#34d399] border-[#34d399]/30 bg-[#34d399]/10",
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

export default function DatasetCard({
  dataset,
  onContextMenu,
}: {
  dataset: Dataset;
  onContextMenu?: (dataset: Dataset, x: number, y: number) => void;
}) {
  const visibleClasses = dataset.classes.slice(0, 4);
  const extra = dataset.classes.length - visibleClasses.length;
  const categoryLabel = CATEGORY_LABELS[dataset.category] ?? dataset.category;
  return (
    <Link
      href={`/datasets/${dataset.id}`}
      onContextMenu={
        onContextMenu
          ? (e) => {
              e.preventDefault();
              onContextMenu(dataset, e.clientX, e.clientY);
            }
          : undefined
      }
      className="glass-card rounded-2xl p-5 flex flex-col justify-between gap-3 transition-all hover:border-brand-500/30 cursor-pointer group"
    >
      <div className="flex items-start justify-between gap-2">
        <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-zinc-900/90 border border-zinc-700/80 text-zinc-300 group-hover:text-brand-400 transition-colors">
          <CategoryIcon category={dataset.category} />
        </span>
        <span className="flex min-w-0 items-center justify-end gap-1.5">
          {dataset.autoTracked && (
            <span className="tracking-caps rounded-full border border-cyan-400/30 bg-cyan-400/10 px-2 py-0.5 text-[10px] font-medium uppercase text-cyan-300" title="AutoTracker">
              AutoTracker
            </span>
          )}
          <span
            className={`tracking-caps truncate rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase ${STATUS_STYLES[dataset.status]}`}
            title={STATUS_LABELS[dataset.status]}
          >
            {STATUS_LABELS[dataset.status]}
          </span>
        </span>
      </div>
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-zinc-100" title={dataset.title}>
          {dataset.title}
        </p>
        <p className="truncate font-mono text-xs text-zinc-500" title={dataset.slug}>
          {dataset.slug}
        </p>
        <p className="tracking-caps mt-1 truncate font-mono text-[10px] uppercase text-zinc-500" title={categoryLabel}>
          {categoryLabel}
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="p-2.5 rounded-xl bg-zinc-900/80 border border-zinc-800/60">
          <p className="tracking-caps text-[10px] uppercase text-zinc-500">
            Imagens
          </p>
          <p className="font-mono text-sm font-bold text-zinc-200">
            {dataset.imagesCount.toLocaleString()}
          </p>
        </div>
        <div className="p-2.5 rounded-xl bg-zinc-900/80 border border-zinc-800/60">
          <p className="tracking-caps text-[10px] uppercase text-zinc-500">
            Rotuladas
          </p>
          <p className="font-mono text-sm font-bold text-[#34d399]">
            {formatPercent(dataset.labeledCount, dataset.imagesCount)}
          </p>
        </div>
      </div>
      {dataset.classes.length > 0 && (
        <div className="mt-3">
          <span className="text-[10px] text-zinc-400 block mb-1 font-mono uppercase tracking-caps">
            Classes ({dataset.classes.length}):
          </span>
          <div className="flex flex-wrap gap-1">
            {visibleClasses.map((c) => (
              <span
                key={c.id}
                title={c.name}
                className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 text-zinc-300 border border-zinc-800 max-w-full truncate"
              >
                {c.name}
              </span>
            ))}
            {extra > 0 && (
              <span title={`Mais ${extra} classes`} className="text-[10px] font-mono px-2 py-0.5 rounded bg-zinc-900 text-zinc-400 border border-zinc-800">
                +{extra}
              </span>
            )}
          </div>
        </div>
      )}
      <div className="mt-5 pt-3 border-t border-zinc-800/80 flex items-center justify-between gap-2 text-[11px] text-zinc-400 font-mono">
        <span className="min-w-0 truncate" title={`${formatBytes(dataset.sizeBytes)} · ${formatRelativeTime(dataset.lastModified)}`}>
          {formatBytes(dataset.sizeBytes)} · {formatRelativeTime(dataset.lastModified)}
        </span>
        <button
          type="button"
          disabled
          title="Treino chega na fatia 4"
          onClick={(e) => e.preventDefault()}
          className="shrink-0 cursor-not-allowed font-medium text-brand-400/70"
        >
          Treinar →
        </button>
      </div>
    </Link>
  );
}
