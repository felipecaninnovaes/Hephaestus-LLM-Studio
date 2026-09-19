"use client";

import { type RefObject, useState } from "react";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { Spinner } from "@/components/ui/Spinner";
import {
  INIT_STRENGTH_MAX,
  INIT_STRENGTH_MIN,
} from "./generationTypes";

export interface GenerationInitImageSectionProps {
  initActive: boolean;
  initUploading: boolean;
  initPreviewUrl: string | null;
  initOriginLabel: string | null;
  initImageMeta: {
    filename: string;
    width: number;
    height: number;
  } | null;
  initGenerationId: string | null;
  initStrength: number;
  setInitStrength: (val: number) => void;
  onClearInit: () => void;
  onInitFile: (file: File) => void;
  initFileRef: RefObject<HTMLInputElement | null>;
  disabled: boolean;
}

export function GenerationInitImageSection({
  initActive,
  initUploading,
  initPreviewUrl,
  initOriginLabel,
  initImageMeta,
  initGenerationId,
  initStrength,
  setInitStrength,
  onClearInit,
  onInitFile,
  initFileRef,
  disabled,
}: GenerationInitImageSectionProps) {
  const [initDragOver, setInitDragOver] = useState(false);

  return (
    <div className="space-y-2 rounded-xl border border-white/8 bg-white/[0.02] p-3">
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
        <span className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Imagem inicial (img2img)
        </span>
        {initActive && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClearInit}
            disabled={disabled}
            aria-label="Remover imagem inicial"
          >
            Limpar
          </Button>
        )}
      </div>

      {!initActive ? (
        <button
          type="button"
          onClick={() => initFileRef.current?.click()}
          disabled={disabled || initUploading}
          aria-label="Enviar imagem inicial para img2img. Pressione Enter para escolher um arquivo PNG, JPEG ou WebP de até 20 MiB."
          onDragOver={(e) => {
            e.preventDefault();
            if (!disabled && !initUploading) setInitDragOver(true);
          }}
          onDragLeave={() => setInitDragOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setInitDragOver(false);
            const f = e.dataTransfer.files?.[0];
            if (f && !disabled && !initUploading) void onInitFile(f);
          }}
          className={`flex min-h-20 w-full cursor-pointer flex-col items-center justify-center gap-1.5 rounded-xl border border-dashed px-3 py-4 text-center transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 disabled:cursor-not-allowed ${
            initDragOver
              ? "border-brand-500/60 bg-brand-500/[0.08]"
              : "border-zinc-700 bg-black/30 hover:border-zinc-500"
          } ${(disabled || initUploading) && "opacity-60"}`}
        >
          {initUploading ? (
            <span className="flex items-center gap-2 font-mono text-2xs text-zinc-300">
              <Spinner className="size-4" />
              Enviando imagem…
            </span>
          ) : (
            <>
              <span className="text-xs text-zinc-300">
                Arraste uma imagem ou clique para escolher
              </span>
              <span className="font-mono text-3xs text-zinc-500">
                PNG, JPEG ou WebP · até 20 MiB
              </span>
            </>
          )}
        </button>
      ) : (
        <div className="flex items-center gap-3 rounded-xl border border-white/8 bg-black/40 p-2">
          {initPreviewUrl && (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={initPreviewUrl}
              alt="Pré-visualização da imagem inicial"
              className="size-14 shrink-0 rounded-lg border border-white/10 object-cover"
            />
          )}
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span
              title={initImageMeta?.filename ?? `Geração ${initGenerationId}`}
              className="truncate font-mono text-2xs text-zinc-200"
            >
              {initImageMeta?.filename ?? "Imagem da galeria"}
            </span>
            <span className="flex flex-wrap items-center gap-1.5">
              {initOriginLabel && (
                <span className="rounded border border-brand-500/20 bg-brand-500/10 px-1.5 py-0.5 font-mono text-4xs text-brand-300">
                  {initOriginLabel}
                </span>
              )}
              <span className="rounded border border-white/5 bg-zinc-900 px-1.5 py-0.5 font-mono text-4xs text-zinc-400">
                {initImageMeta
                  ? `${initImageMeta.width}×${initImageMeta.height}`
                  : "dimensões do alvo"}
              </span>
            </span>
          </div>
        </div>
      )}

      <label htmlFor="gen-init-file" className="sr-only">
        Escolher arquivo de imagem inicial (PNG, JPEG ou WebP, até 20 MiB)
      </label>
      <input
        ref={initFileRef}
        id="gen-init-file"
        type="file"
        accept="image/png,image/jpeg,image/webp"
        className="sr-only"
        disabled={disabled || initUploading}
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (f) void onInitFile(f);
          e.target.value = "";
        }}
      />
      {!initActive && !initUploading && (
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={() => initFileRef.current?.click()}
          disabled={disabled}
          className="w-full font-mono text-2xs"
        >
          Escolher arquivo…
        </Button>
      )}

      {initActive && (
        <div className="pt-1 space-y-1">
          <Slider
            label="Influência da imagem inicial (Denoise)"
            value={initStrength}
            onChange={setInitStrength}
            min={INIT_STRENGTH_MIN}
            max={INIT_STRENGTH_MAX}
            step={0.05}
            disabled={disabled}
            formatValue={(v) =>
              `${Math.round(v * 100)}% · ${
                v <= 0.35
                  ? "preserva muito"
                  : v >= 0.75
                    ? "transformação forte"
                    : "equilibrado"
              }`
            }
          />
          <p className="font-mono text-4xs text-zinc-400 leading-tight">
            Valores menores preservam a estrutura original; valores maiores dão
            mais liberdade ao prompt.
          </p>
        </div>
      )}
    </div>
  );
}
