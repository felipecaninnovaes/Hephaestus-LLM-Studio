import React, { forwardRef, type HTMLAttributes } from "react";

export type BadgeVariant =
  | "ready"
  | "alert"
  | "info"
  | "danger"
  | "telemetry"
  | "brand"
  | "mono";

export function jobStatusToBadgeVariant(status: string): BadgeVariant {
  switch (status) {
    case "done":
      return "ready";
    case "running":
      return "brand";
    case "queued":
    case "cancelling":
      return "alert";
    case "failed":
      return "danger";
    case "cancelled":
    default:
      return "mono";
  }
}

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: BadgeVariant;
  pulse?: boolean;
  dot?: boolean;
}

const VARIANT_CLASSES: Record<BadgeVariant, string> = {
  ready:
    "border-[#34d399]/30 bg-[#34d399]/10 text-[#34d399] uppercase tracking-caps rounded-full px-2 py-0.5 text-[10px]",
  alert:
    "border-amber-400/30 bg-amber-400/10 text-amber-300 uppercase tracking-caps rounded-full px-2 py-0.5 text-[10px]",
  info:
    "border-cyan-400/30 bg-cyan-400/10 text-cyan-300 uppercase tracking-caps rounded-full px-2 py-0.5 text-[10px]",
  danger:
    "border-rose-500/30 bg-rose-500/10 text-rose-300 uppercase tracking-caps rounded-full px-2 py-0.5 text-[10px]",
  telemetry:
    "border-white/10 bg-black/40 text-zinc-300 rounded-full px-2.5 py-1 text-[11px]",
  brand:
    "border-brand-500/35 bg-brand-500/10 text-brand-400 uppercase tracking-caps rounded-full px-3 py-1 text-[11px]",
  mono:
    "border-zinc-800 bg-zinc-900 text-zinc-300 rounded px-2 py-0.5 text-[10px]",
};

export const Badge = forwardRef<HTMLSpanElement, BadgeProps>(
  (
    {
      variant = "mono",
      pulse = false,
      dot = false,
      className = "",
      children,
      ...props
    },
    ref,
  ) => {
    const variantClass = VARIANT_CLASSES[variant];

    return (
      <span
        ref={ref}
        className={`inline-flex items-center gap-1.5 font-mono font-medium border backdrop-blur-sm ${variantClass} ${className}`.trim()}
        {...props}
      >
        {pulse && (
          <span
            className="size-1.5 shrink-0 rounded-full bg-current animate-pulse"
            aria-hidden="true"
          />
        )}
        {!pulse && dot && (
          <span
            className="size-1.5 shrink-0 rounded-full bg-current"
            aria-hidden="true"
          />
        )}
        {children}
      </span>
    );
  },
);

Badge.displayName = "Badge";
export default Badge;
