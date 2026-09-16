"use client";

import React, { forwardRef, type InputHTMLAttributes } from "react";

export interface SliderProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> {
  label?: string;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  formatValue?: (value: number) => string;
}

export const Slider = forwardRef<HTMLInputElement, SliderProps>(
  (
    {
      label,
      value,
      onChange,
      min = 0,
      max = 100,
      step = 1,
      formatValue,
      disabled,
      className = "",
      ...props
    },
    ref,
  ) => {
    const formatted = formatValue ? formatValue(value) : String(value);

    return (
      <div className={`w-full ${className}`.trim()}>
        {(label || formatValue) && (
          <div className="mb-1.5 flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5 font-mono text-2xs">
            {label && (
              <span className="tracking-caps font-medium uppercase text-zinc-300">
                {label}
              </span>
            )}
            <span className="shrink-0 font-bold text-zinc-100">{formatted}</span>
          </div>
        )}
        <input
          ref={ref}
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(parseFloat(e.target.value))}
          className="w-full h-2 cursor-pointer touch-manipulation appearance-none rounded-lg bg-zinc-800 accent-brand-500 disabled:opacity-55 disabled:cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
          {...props}
        />
      </div>
    );
  },
);

Slider.displayName = "Slider";
export default Slider;
