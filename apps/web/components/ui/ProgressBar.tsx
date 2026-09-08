import React, { forwardRef, type HTMLAttributes } from "react";

export type ProgressVariant =
  | "success"
  | "brand"
  | "amber"
  | "rose"
  | "cyan";

export interface ProgressBarProps extends HTMLAttributes<HTMLDivElement> {
  value: number; // 0 to 100
  variant?: ProgressVariant;
  size?: "sm" | "md" | "lg";
  label?: string;
  showPercent?: boolean;
}

const VARIANT_FILLS: Record<ProgressVariant, string> = {
  success: "bg-[#34d399]",
  brand: "bg-brand-500",
  amber: "bg-amber-400",
  rose: "bg-rose-500",
  cyan: "bg-cyan-400",
};

const SIZE_CLASSES = {
  sm: "h-1",
  md: "h-1.5",
  lg: "h-2",
};

export const ProgressBar = forwardRef<HTMLDivElement, ProgressBarProps>(
  (
    {
      value,
      variant = "brand",
      size = "md",
      label,
      showPercent = false,
      className = "",
      ...props
    },
    ref,
  ) => {
    const clamped = Math.min(100, Math.max(0, isNaN(value) ? 0 : value));
    const fillClass = VARIANT_FILLS[variant];
    const sizeClass = SIZE_CLASSES[size];

    return (
      <div ref={ref} className={`w-full ${className}`.trim()} {...props}>
        {(label || showPercent) && (
          <div className="mb-1 flex items-center justify-between font-mono text-[10px] text-zinc-400">
            {label && <span>{label}</span>}
            {showPercent && (
              <span className="text-zinc-200 font-medium">
                {clamped.toFixed(0)}%
              </span>
            )}
          </div>
        )}
        <div
          role="progressbar"
          aria-valuenow={clamped}
          aria-valuemin={0}
          aria-valuemax={100}
          className={`w-full overflow-hidden rounded-full bg-zinc-800 ${sizeClass}`}
        >
          <div
            className={`h-full rounded-full transition-all duration-500 ${fillClass}`}
            style={{ width: `${clamped}%` }}
          />
        </div>
      </div>
    );
  },
);

ProgressBar.displayName = "ProgressBar";
export default ProgressBar;
