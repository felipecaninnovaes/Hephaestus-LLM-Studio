"use client";

import type React from "react";
import { useCallback, useRef, useState } from "react";
import { IconSliders } from "@/components/icons";

export interface ImageSplitCompareProps {
  /** Imagem base (lado esquerdo / antes) */
  beforeSrc: string;
  beforeAlt?: string;
  beforeLabel?: string;
  /** Imagem de comparação (lado direito / depois) */
  afterSrc: string;
  afterAlt?: string;
  afterLabel?: string;
  /** Orientação do divisor: horizontal (esquerda/direita) ou vertical (cima/baixo) */
  orientation?: "horizontal" | "vertical";
  /** Posição inicial (0 a 100). Default: 50 */
  initialPosition?: number;
  /** Callback ao mover o divisor */
  onPositionChange?: (position: number) => void;
  className?: string;
  aspectRatioClass?: string;
}

/**
 * ImageSplitCompare
 *
 * Comparador split antes-e-depois interativo:
 * - Imagens sobrepostas no mesmo quadro com object-contain alinhado.
 * - Divisor arrastável por mouse e touch via Pointer Events com pointer capture.
 * - Suporte a teclado acessível (setas, Home, End) com role="slider".
 * - Suporte a orientação horizontal (A|B) e vertical (A sobre B).
 */
export function ImageSplitCompare({
  beforeSrc,
  beforeAlt = "Amostra A",
  beforeLabel = "A",
  afterSrc,
  afterAlt = "Amostra B",
  afterLabel = "B",
  orientation = "horizontal",
  initialPosition = 50,
  onPositionChange,
  className = "",
  aspectRatioClass = "aspect-square",
}: ImageSplitCompareProps) {
  const [position, setPosition] = useState(() =>
    Math.min(Math.max(initialPosition, 0), 100),
  );
  const [isDragging, setIsDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const isHorizontal = orientation === "horizontal";

  const updatePosition = useCallback(
    (clientX: number, clientY: number) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      let rawPos: number;
      if (isHorizontal) {
        rawPos = ((clientX - rect.left) / rect.width) * 100;
      } else {
        rawPos = ((clientY - rect.top) / rect.height) * 100;
      }
      const clamped = Math.min(Math.max(rawPos, 0), 100);
      const rounded = Math.round(clamped * 10) / 10;
      setPosition(rounded);
      onPositionChange?.(rounded);
    },
    [isHorizontal, onPositionChange],
  );

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // Apenas botão primário (toque ou botão esquerdo do mouse)
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setIsDragging(true);
    updatePosition(e.clientX, e.clientY);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!isDragging) return;
    updatePosition(e.clientX, e.clientY);
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (isDragging) {
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        // Ignora se o elemento já tiver liberado a captura
      }
      setIsDragging(false);
    }
  };

  const handleLostPointerCapture = () => {
    setIsDragging(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    let delta = 0;
    const step = e.shiftKey ? 10 : 2;

    if (isHorizontal) {
      if (e.key === "ArrowLeft" || e.key === "ArrowDown") delta = -step;
      else if (e.key === "ArrowRight" || e.key === "ArrowUp") delta = step;
    } else {
      if (e.key === "ArrowUp" || e.key === "ArrowLeft") delta = -step;
      else if (e.key === "ArrowDown" || e.key === "ArrowRight") delta = step;
    }

    if (e.key === "Home") {
      e.preventDefault();
      setPosition(0);
      onPositionChange?.(0);
      return;
    }
    if (e.key === "End") {
      e.preventDefault();
      setPosition(100);
      onPositionChange?.(100);
      return;
    }

    if (delta !== 0) {
      e.preventDefault();
      const next = Math.min(Math.max(position + delta, 0), 100);
      setPosition(next);
      onPositionChange?.(next);
    }
  };

  // Clippings usando clip-path moderno para suportar tanto horizontal quanto vertical
  // Lado Before (A): visível até a posição do divisor
  const beforeClipPath = isHorizontal
    ? `polygon(0 0, ${position}% 0, ${position}% 100%, 0 100%)`
    : `polygon(0 0, 100% 0, 100% ${position}%, 0 ${position}%)`;

  return (
    <div
      ref={containerRef}
      role="slider"
      aria-label="Comparador visual A/B"
      aria-valuenow={Math.round(position)}
      aria-valuemin={0}
      aria-valuemax={100}
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
      onLostPointerCapture={handleLostPointerCapture}
      className={`group relative select-none overflow-hidden rounded-xl border border-white/10 bg-black/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 touch-none ${
        isHorizontal ? "cursor-ew-resize" : "cursor-ns-resize"
      } ${aspectRatioClass} ${className}`.trim()}
    >
      {/* Camada B (After - Fundo completo) */}
      <div className="absolute inset-0 size-full">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={afterSrc}
          alt={afterAlt}
          className="size-full object-contain pointer-events-none"
          draggable={false}
        />
      </div>

      {/* Camada A (Before - Recortada pelo clip-path) */}
      <div
        className="absolute inset-0 size-full pointer-events-none"
        style={{ clipPath: beforeClipPath }}
      >
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={beforeSrc}
          alt={beforeAlt}
          className="size-full object-contain pointer-events-none"
          draggable={false}
        />
      </div>

      {/* Badges dos cantos identificando A e B */}
      <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex items-center justify-between p-2.5">
        <span className="inline-flex items-center rounded-md border border-white/15 bg-black/75 px-2 py-0.5 font-mono text-3xs font-semibold text-brand-300 shadow-sm backdrop-blur-md">
          {beforeLabel}
        </span>
        <span className="inline-flex items-center rounded-md border border-white/15 bg-black/75 px-2 py-0.5 font-mono text-3xs font-semibold text-zinc-300 shadow-sm backdrop-blur-md">
          {afterLabel}
        </span>
      </div>

      {/* Linha divisora */}
      {isHorizontal ? (
        <div
          className="pointer-events-none absolute inset-y-0 w-0.5 bg-brand-400/90 shadow-[0_0_8px_rgba(131,80,242,0.6)]"
          style={{ left: `${position}%`, transform: "translateX(-50%)" }}
        />
      ) : (
        <div
          className="pointer-events-none absolute inset-x-0 h-0.5 bg-brand-400/90 shadow-[0_0_8px_rgba(131,80,242,0.6)]"
          style={{ top: `${position}%`, transform: "translateY(-50%)" }}
        />
      )}

      {/* Handle central arrastável */}
      <div
        className={`pointer-events-none absolute z-20 flex size-8 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full border border-white/30 bg-black/85 text-brand-300 shadow-lg shadow-black/60 backdrop-blur-md transition-transform group-hover:scale-110 ${
          isDragging ? "scale-110 border-brand-400 ring-2 ring-brand-400/40" : ""
        }`}
        style={
          isHorizontal
            ? { left: `${position}%`, top: "50%" }
            : { left: "50%", top: `${position}%` }
        }
      >
        <IconSliders className={`size-4 ${!isHorizontal ? "rotate-90" : ""}`} />
      </div>
    </div>
  );
}
