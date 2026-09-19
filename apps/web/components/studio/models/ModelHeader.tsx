"use client";

import { IconBox, IconDownload, IconPlus } from "@/components/icons";
import { Button } from "@/components/ui/Button";

export interface ModelHeaderProps {
  onOpenUpload: () => void;
  onOpenDownload: () => void;
}

export function ModelHeader({
  onOpenUpload,
  onOpenDownload,
}: ModelHeaderProps) {
  return (
    <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between border-b border-white/5 pb-5">
      <div className="flex items-center space-x-3">
        <div className="flex size-10 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm text-brand-400">
          <IconBox className="size-5" />
        </div>
        <div>
          <h1 className="font-display text-lg font-bold tracking-tight text-white sm:text-xl">
            Modelos & Pesos
          </h1>
          <p className="font-mono text-2xs text-zinc-400">
            Checkpoints de visão computacional (YOLO, YOLO-World, CLIP) e
            difusão treinados ou importados
          </p>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={onOpenDownload}
        >
          <IconDownload className="h-3.5 w-3.5" />
          <span>Baixar por URL</span>
        </Button>
        <Button
          type="button"
          variant="primary"
          size="sm"
          onClick={onOpenUpload}
        >
          <IconPlus className="h-3.5 w-3.5" />
          <span>Enviar Pesos</span>
        </Button>
      </div>
    </div>
  );
}
