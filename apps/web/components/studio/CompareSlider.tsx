"use client";

import type React from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Modal } from "@/components/ui";
import { TruncatedText } from "@/components/ui";
import type { Generation } from "@/types/studio";
import { usePortalRoot } from "@/hooks/usePortalRoot";

/* ═══════════════════════════════════════════════════════════════════
   CompareSlider — comparador side-by-side de 2 imagens (G.8 D6)
   Modal full-bleed com clip-path reveal + handle arrastável.
   ═══════════════════════════════════════════════════════════════════ */

export interface CompareSliderProps {
  open: boolean;
  onClose: () => void;
  imageA: Generation;
  imageB: Generation;
  getFullImageUrl: (gen: Generation) => string;
}

export default function CompareSlider({
  open,
  onClose,
  imageA,
  imageB,
  getFullImageUrl,
}: CompareSliderProps) {
  const portalRoot = usePortalRoot();
  const [position, setPosition] = useState(50);
  const containerRef = useRef<HTMLDivElement>(null);
  const isDragging = useRef(false);

  /* ── Pointer handlers ── */
  const updatePosition = useCallback((clientX: number) => {
    const el = containerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const x = clientX - rect.left;
    const pct = Math.max(0, Math.min(100, (x / rect.width) * 100));
    setPosition(pct);
  }, []);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      isDragging.current = true;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      updatePosition(e.clientX);
    },
    [updatePosition],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!isDragging.current) return;
      updatePosition(e.clientX);
    },
    [updatePosition],
  );

  const handlePointerUp = useCallback(() => {
    isDragging.current = false;
  }, []);

  /* ── Keyboard navigation (a11y) ── */
  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "ArrowLeft") {
      setPosition((p) => Math.max(0, p - 2));
      e.preventDefault();
    } else if (e.key === "ArrowRight") {
      setPosition((p) => Math.min(100, p + 2));
      e.preventDefault();
    }
  }, []);

  /* ── Focus trap: Escape closes ── */
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open || !portalRoot) return null;

  const urlA = getFullImageUrl(imageA);
  const urlB = getFullImageUrl(imageB);
  const seedA = imageA.seed;
  const seedB = imageB.seed;
  const baseA = String(imageA.params?.base_model || imageA.params?.baseModel || "—");
  const baseB = String(imageB.params?.base_model || imageB.params?.baseModel || "—");

  return createPortal(
    // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: backdrop suplementar — o fechamento por teclado é global (Escape) e há botão fechar explícito; o backdrop fica fora da tab-order de propósito.
    <div
      className="fixed inset-0 z-modal flex flex-col items-center justify-center bg-black/80 p-4 backdrop-blur-sm"
      onClick={onClose}
    >
      {/* Header */}
      <div className="flex w-full max-w-4xl items-center justify-between mb-3 px-2">
        <span className="font-mono text-xs text-zinc-400">
          seed {seedA} · {baseA}
        </span>
        <span className="font-mono text-3xs text-zinc-500">Comparador</span>
        <span className="font-mono text-xs text-zinc-400">
          seed {seedB} · {baseB}
        </span>
      </div>

      {/* Compare area */}
      <div
        ref={containerRef}
        role="slider"
        tabIndex={0}
        aria-label="Comparador de imagens"
        aria-valuenow={Math.round(position)}
        aria-valuemin={0}
        aria-valuemax={100}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onKeyDown={handleKeyDown}
        className="relative w-full max-w-4xl overflow-hidden rounded-2xl border border-white/10 bg-zinc-900 select-none"
        style={{ touchAction: "none", maxHeight: "70vh", overscrollBehavior: "contain" }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Image B (base — fundo) */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={urlB}
          alt={`Seed ${seedB}`}
          className="w-full h-auto object-contain max-h-[70vh]"
          draggable={false}
        />

        {/* Image A (topo — clip reveal) */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={urlA}
          alt={`Seed ${seedA}`}
          className="absolute inset-0 w-full h-auto object-contain max-h-[70vh]"
          style={{ clipPath: `inset(0 ${100 - position}% 0 0)` }}
          draggable={false}
        />

        {/* Handle vertical */}
        <div
          className="absolute top-0 bottom-0 z-10 flex items-center justify-center"
          style={{ left: `${position}%`, transform: "translateX(-50%)" }}
          aria-hidden="true"
        >
          {/* Linha vertical */}
          <div className="absolute top-0 bottom-0 w-px bg-white/70" />
          {/* Handle pill — ≥40px touch target */}
          <div className="relative flex size-10 items-center justify-center rounded-full border-2 border-white bg-black/60 backdrop-blur-md shadow-lg cursor-ew-resize">
            <div className="flex gap-1">
              <div className="h-3 w-0.5 rounded-full bg-white/70" />
              <div className="h-3 w-0.5 rounded-full bg-white/70" />
            </div>
          </div>
        </div>
      </div>

      {/* Prompts below */}
      <div className="flex w-full max-w-4xl gap-4 mt-3 px-2">
        <div className="flex-1 min-w-0">
          <span className="font-mono text-3xs text-zinc-500 uppercase tracking-caps block mb-0.5">
            Prompt A (esquerda)
          </span>
          <TruncatedText
            text={imageA.prompt}
            lines={2}
            as="p"
            className="text-xs text-zinc-300"
          />
        </div>
        <div className="flex-1 min-w-0">
          <span className="font-mono text-3xs text-zinc-500 uppercase tracking-caps block mb-0.5">
            Prompt B (direita)
          </span>
          <TruncatedText
            text={imageB.prompt}
            lines={2}
            as="p"
            className="text-xs text-zinc-300"
          />
        </div>
      </div>

      {/* Close button */}
      <button
        type="button"
        onClick={onClose}
        className="absolute top-4 right-4 flex size-10 items-center justify-center rounded-lg border border-white/20 bg-black/60 text-zinc-300 hover:bg-black/80 hover:text-white transition-colors cursor-pointer"
        aria-label="Fechar comparador"
      >
        ✕
      </button>
    </div>
  , portalRoot);
}
