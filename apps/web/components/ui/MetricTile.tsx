import React, { forwardRef, type HTMLAttributes } from "react";

export interface MetricTileProps extends HTMLAttributes<HTMLDivElement> {
  label: string;
  value: string | number;
  highlightColor?: "default" | "success" | "brand" | "amber" | "rose" | "cyan";
  subtext?: string;
}

const VALUE_COLORS = {
  default: "text-zinc-200",
  success: "text-status-success",
  brand: "text-brand-300",
  amber: "text-amber-300",
  rose: "text-rose-300",
  cyan: "text-cyan-300",
};

export const MetricTile = forwardRef<HTMLDivElement, MetricTileProps>(
  (
    {
      label,
      value,
      highlightColor = "default",
      subtext,
      className = "",
      ...props
    },
    ref,
  ) => {
    const valueColorClass = VALUE_COLORS[highlightColor];

    return (
      <div
        ref={ref}
        className={`rounded-xl border border-zinc-800/60 bg-zinc-900/80 backdrop-blur-sm p-2.5 ${className}`.trim()}
        {...props}
      >
        <p className="tracking-caps font-mono text-3xs uppercase text-zinc-500 truncate">
          {label}
        </p>
        <p className={`font-mono text-sm font-bold truncate ${valueColorClass}`}>
          {value}
        </p>
        {subtext && (
          <p className="mt-0.5 font-mono text-3xs text-zinc-500 truncate">
            {subtext}
          </p>
        )}
      </div>
    );
  },
);

MetricTile.displayName = "MetricTile";
export default MetricTile;
