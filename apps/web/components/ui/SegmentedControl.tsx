"use client";

import React from "react";

export interface SegmentedControlOption<T extends string = string> {
  id: T;
  label?: string;
  icon?: React.ReactNode;
  title?: string;
  ariaLabel?: string;
}

export interface SegmentedControlProps<T extends string = string> {
  options: SegmentedControlOption<T>[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel?: string;
  className?: string;
}

export function SegmentedControl<T extends string = string>({
  options,
  value,
  onChange,
  ariaLabel = "Opções",
  className = "",
}: SegmentedControlProps<T>) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className={`inline-flex rounded-full border border-white/10 bg-black/40 p-1 ${className}`.trim()}
    >
      {options.map((opt) => {
        const active = opt.id === value;
        const titleText = opt.title || opt.label;

        return (
          <button
            key={opt.id}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={opt.ariaLabel || opt.label || opt.id}
            title={titleText}
            onClick={() => onChange(opt.id)}
            className={`inline-flex h-7 items-center justify-center gap-1.5 rounded-full px-2.5 text-xs transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer ${
              active
                ? "bg-brand-500/[0.18] text-brand-300 font-medium shadow-sm"
                : "text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
            }`}
          >
            {opt.icon}
            {opt.label && <span>{opt.label}</span>}
          </button>
        );
      })}
    </div>
  );
}

export default SegmentedControl;
