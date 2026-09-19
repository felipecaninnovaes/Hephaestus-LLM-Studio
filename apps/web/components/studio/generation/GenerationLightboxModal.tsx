"use client";

import { IconDownload } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import {
  type GeneratedImageItem,
  UPSCALE_MODEL_SHORT_LABEL,
} from "./generationTypes";

export interface GenerationLightboxModalProps {
  open: boolean;
  onClose: () => void;
  item: GeneratedImageItem | null;
  onDownload: () => void;
}

export function GenerationLightboxModal({
  open,
  onClose,
  item,
  onDownload,
}: GenerationLightboxModalProps) {
  if (!item) return null;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Visualização"
      maxWidth="xl"
      bodyClassName="flex flex-col items-center gap-4"
    >
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        src={item.imageUrl}
        alt={item.prompt}
        className="max-h-[60vh] w-auto max-w-full object-contain rounded-xl border border-white/10"
      />
      <div className="w-full flex items-center justify-between gap-4 px-1">
        <div className="flex min-w-0 flex-col gap-1.5">
          <p className="text-xs text-zinc-300 truncate max-w-md italic">
            &ldquo;{item.prompt}&rdquo;
          </p>
          {item.upscale && (
            <span className="font-mono text-4xs px-1.5 py-0.5 rounded bg-brand-500/10 border border-brand-500/20 text-brand-300 w-fit">
              upscale{" "}
              {UPSCALE_MODEL_SHORT_LABEL[item.upscale.model] ??
                item.upscale.model}{" "}
              {item.upscale.scale}x
            </span>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Button size="sm" variant="primary" onClick={onDownload}>
            <IconDownload className="size-3.5 mr-1" />
            Baixar
          </Button>
          <Button size="sm" variant="ghost" onClick={onClose}>
            Fechar
          </Button>
        </div>
      </div>
    </Modal>
  );
}
