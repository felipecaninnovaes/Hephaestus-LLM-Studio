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
  ready: "text-emerald-300 border-emerald-400/30 bg-emerald-400/10",
};

function CategoryIcon({ category }: { category: Dataset["category"] }) {
  if (category === "yolo")
    return <IconTarget className="w-4 h-4 text-emerald-400" />;
  if (category === "difusao")
    return <IconSparkles className="w-4 h-4 text-violet-400" />;
  return <IconLayers className="w-4 h-4 text-sky-400" />;
}

export default function DatasetCard({ dataset }: { dataset: Dataset }) {
  const visibleClasses = dataset.classes.slice(0, 4);
  const extra = dataset.classes.length - visibleClasses.length;
  return (
    <Link
      href={`/datasets/${dataset.id}`}
      className="glass-card rounded-2xl p-4 flex flex-col gap-3 transition-colors hover:border-zinc-500/40"
    >
      <div className="flex items-start justify-between gap-2">
        <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-zinc-800/80">
          <CategoryIcon category={dataset.category} />
        </span>
        <span
          className={`tracking-caps rounded-full border px-2 py-0.5 text-[10px] font-medium uppercase ${STATUS_STYLES[dataset.status]}`}
        >
          {STATUS_LABELS[dataset.status]}
        </span>
      </div>
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-zinc-100">
          {dataset.title}
        </p>
        <p className="truncate font-mono text-xs text-zinc-500">
          {dataset.slug}
        </p>
      </div>
      <div className="grid grid-cols-2 gap-2">
        <div className="rounded-lg bg-zinc-900/60 px-2.5 py-2">
          <p className="tracking-caps text-[10px] uppercase text-zinc-500">
            Imagens
          </p>
          <p className="font-mono text-sm text-zinc-100">
            {dataset.imagesCount}
          </p>
        </div>
        <div className="rounded-lg bg-zinc-900/60 px-2.5 py-2">
          <p className="tracking-caps text-[10px] uppercase text-zinc-500">
            Rotulado
          </p>
          <p className="font-mono text-sm text-zinc-100">
            {formatPercent(dataset.labeledCount, dataset.imagesCount)}
          </p>
        </div>
      </div>
      {dataset.classes.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {visibleClasses.map((c) => (
            <span
              key={c.id}
              className="inline-flex items-center gap-1.5 rounded-full bg-zinc-900/60 px-2 py-0.5 text-[11px] text-zinc-300"
            >
              <span
                className="h-2 w-2 rounded-full"
                style={{ background: c.color }}
              />
              {c.name}
            </span>
          ))}
          {extra > 0 && (
            <span className="rounded-full bg-zinc-900/60 px-2 py-0.5 font-mono text-[11px] text-zinc-400">
              +{extra}
            </span>
          )}
        </div>
      )}
      <div className="mt-auto flex items-center justify-between gap-2 pt-1">
        <p className="truncate font-mono text-[11px] text-zinc-500">
          {formatBytes(dataset.sizeBytes)} ·{" "}
          {formatRelativeTime(dataset.lastModified)}
        </p>
        <button
          type="button"
          disabled
          title="Treino chega na fatia 4"
          onClick={(e) => e.preventDefault()}
          className="shrink-0 rounded-lg border border-zinc-700/80 bg-zinc-900/60 px-3 py-1.5 text-xs font-medium text-zinc-300"
        >
          Treinar
        </button>
      </div>
    </Link>
  );
}
