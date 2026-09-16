"use client";

import React from "react";
import { Button } from "./Button";
import { IconZoomIn, IconZoomOut } from "@/components/icons";

export interface ZoomControlProps {
  value: number;
  onChange: (value: number | ((prev: number) => number)) => void;
  min?: number;
  max?: number;
  step?: number;
  resetValue?: number;
  className?: string;
}

export function ZoomControl({
  value,
  onChange,
  min = 50,
  max = 250,
  step = 25,
  resetValue = 100,
  className = "",
}: ZoomControlProps) {
  const handleZoomOut = () => {
    onChange((z) => Math.max(min, z - step));
  };

  const handleZoomIn = () => {
    onChange((z) => Math.min(max, z + step));
  };

  const handleReset = () => {
    onChange(resetValue);
  };

  return (
    <div
      role="toolbar"
      aria-label="Controle de zoom do canvas"
      className={`glass-menu flex items-center space-x-2 rounded-xl border border-zinc-800 bg-zinc-900/90 px-3 py-1.5 font-mono text-xs backdrop-blur-sm ${className}`}
    >
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={handleZoomOut}
        disabled={value <= min}
        aria-label="Diminuir zoom do canvas"
        title="Diminuir Zoom"
      >
        <IconZoomOut />
      </Button>
      <span className="min-w-[45px] text-center text-zinc-300 select-none tabular-nums">
        {value}%
      </span>
      <Button
        type="button"
        variant="ghost"
        size="icon-sm"
        onClick={handleZoomIn}
        disabled={value >= max}
        aria-label="Aumentar zoom do canvas"
        title="Aumentar Zoom"
      >
        <IconZoomIn />
      </Button>
      <div className="h-3 w-px bg-zinc-700" aria-hidden="true" />
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={handleReset}
        disabled={value === resetValue}
        className="text-3xs"
        title="Restaurar zoom para 100%"
      >
        Resetar 100%
      </Button>
    </div>
  );
}

export default ZoomControl;
