"use client";

import { IconDownload, IconPencil, IconTrash } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { GlassCard } from "@/components/ui/GlassCard";
import { formatBytes, formatRelativeTime } from "@/lib/format";
import {
  modelSourceLabel,
  type Model,
  type ModelSource,
} from "@/types/studio";

export interface ModelCardProps {
  model: Model;
  onDownload: (m: Model) => void;
  onRename: (m: Model) => void;
  onDelete: (m: Model) => void;
}

const SOURCE_BADGE_CLASSES: Record<ModelSource, string> = {
  train: "border-brand-500/35 bg-brand-500/10 text-brand-400",
  upload: "border-white/15 bg-white/[0.06] text-zinc-300",
  download: "border-white/15 bg-white/[0.06] text-zinc-300",
};

function engineBadge(engine: string) {
  switch (engine) {
    case "world":
      return {
        label: "YOLO-World",
        className: "border-sky-500/35 bg-sky-500/10 text-sky-400",
      };
    case "diffusion":
      return {
        label: "Difusão",
        className: "border-purple-500/35 bg-purple-500/10 text-purple-400",
      };
    case "clip":
      return {
        label: "CLIP",
        className:
          "border-status-success/35 bg-status-success/10 text-status-success",
      };
    default:
      return {
        label: "YOLO",
        className: "border-zinc-700/60 bg-zinc-800/40 text-zinc-300",
      };
  }
}

function formatBadge(name: string): string | null {
  if (name.endsWith(".safetensors")) {
    return "safetensors";
  }
  if (name.endsWith(".pt")) {
    return ".pt";
  }
  return null;
}

export function ModelCard({
  model: m,
  onDownload,
  onRename,
  onDelete,
}: ModelCardProps) {
  const badge = engineBadge(m.engine);
  const format = formatBadge(m.name);

  return (
    <GlassCard interactive className="p-4">
      {/* Header do card */}
      <div className="flex items-start justify-between gap-2 mb-3">
        <div className="min-w-0 flex-1">
          <h3
            className="font-mono text-sm font-semibold text-zinc-100 truncate"
            title={m.name}
          >
            {m.name}
          </h3>
          <div className="mt-1 flex items-center gap-1.5 flex-wrap">
            <span
              className={`inline-flex items-center rounded px-1.5 py-0.5 font-mono text-3xs font-medium uppercase tracking-[0.06em] border ${badge.className}`}
            >
              {badge.label}
            </span>
            {format && (
              <span className="inline-flex items-center rounded border border-zinc-700/60 bg-zinc-800/60 px-1.5 py-0.5 font-mono text-3xs text-zinc-400">
                {format}
              </span>
            )}
            {m.model && (
              <span className="font-mono text-2xs text-zinc-500">
                · {m.model}
              </span>
            )}
          </div>
        </div>
        <span
          className={`shrink-0 rounded-full border px-2 py-0.5 font-mono text-3xs uppercase tracking-[0.06em] ${SOURCE_BADGE_CLASSES[m.source]}`}
          title={`Origem: ${modelSourceLabel(m.source)}`}
        >
          {modelSourceLabel(m.source)}
        </span>
      </div>

      {/* Metadados */}
      <div className="space-y-1.5 text-2xs">
        <div className="flex items-center justify-between">
          <span className="text-zinc-500">Tamanho</span>
          <span className="font-mono text-zinc-300">
            {formatBytes(m.bytes)}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-zinc-500">Criado</span>
          <span className="font-mono text-zinc-300">
            {formatRelativeTime(m.createdAt)}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-zinc-500">MD5</span>
          <span
            className="font-mono text-zinc-400 truncate max-w-[140px]"
            title={m.md5}
          >
            {m.md5}
          </span>
        </div>
      </div>

      {/* Ações */}
      <div className="mt-3 pt-3 border-t border-zinc-800/80 flex items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          className="flex-1"
          onClick={() => onDownload(m)}
          disabled={!m.url}
          title={
            m.url
              ? `Baixar ${m.name}`
              : "Download indisponível — sem URL presigned"
          }
        >
          <IconDownload className="h-3.5 w-3.5" />
          Baixar
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/60"
          onClick={() => onRename(m)}
          title={`Renomear ${m.name}`}
          aria-label={`Renomear modelo ${m.name}`}
        >
          <IconPencil className="h-3.5 w-3.5" />
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="text-zinc-400 hover:text-red-400 hover:bg-red-500/10"
          onClick={() => onDelete(m)}
          title={`Excluir ${m.name}`}
          aria-label={`Excluir modelo ${m.name}`}
        >
          <IconTrash className="h-3.5 w-3.5" />
        </Button>
      </div>
    </GlassCard>
  );
}
