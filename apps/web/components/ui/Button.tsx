"use client";

import React, { forwardRef, type ButtonHTMLAttributes } from "react";

export type ButtonVariant =
  | "primary"
  | "secondary"
  | "ghost"
  | "destructive"
  | "warning";

export type ButtonSize = "sm" | "md" | "lg" | "icon" | "icon-sm";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
}

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
  // The One CTA Rule: outline-violeta translúcido com inset highlight superior
  primary:
    "border border-brand-500/30 bg-brand-500/[0.12] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-brand-500/50 hover:bg-brand-500/[0.18]",
  // Secundário glass translúcido neutro
  secondary:
    "border border-white/10 bg-white/[0.05] text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] hover:border-white/20 hover:bg-white/[0.10]",
  // Ghost sutil sem borda
  ghost:
    "border border-transparent bg-transparent text-zinc-300 hover:bg-white/[0.06] hover:text-white",
  // Destrutivo com borda e véu vermelho translúcido
  destructive:
    "border border-[#ef4444]/30 bg-[#ef4444]/[0.12] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18]",
  // Warning / Alerta com borda e véu âmbar
  warning:
    "border border-amber-500/30 bg-amber-500/[0.12] text-amber-200 shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-amber-500/50 hover:bg-amber-500/[0.18]",
};

const SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: "h-8 px-3 rounded-md text-xs",
  md: "h-9 px-4 rounded-lg text-xs",
  lg: "h-10 px-5 rounded-lg text-sm font-semibold",
  icon: "size-9 p-0 rounded-lg justify-center",
  "icon-sm": "size-8 p-0 rounded-md justify-center",
};

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      variant = "secondary",
      size = "md",
      loading = false,
      leftIcon,
      rightIcon,
      disabled,
      className = "",
      children,
      ...props
    },
    ref,
  ) => {
    const baseClasses =
      "inline-flex items-center justify-center gap-2 font-medium whitespace-nowrap select-none transition active:scale-[0.985] backdrop-blur-sm focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer";

    const variantClass = VARIANT_CLASSES[variant];
    const sizeClass = SIZE_CLASSES[size];

    return (
      <button
        ref={ref}
        disabled={disabled || loading}
        className={`${baseClasses} ${variantClass} ${sizeClass} ${className}`.trim()}
        {...props}
      >
        {loading ? (
          <span
            className="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
            aria-hidden="true"
          />
        ) : (
          leftIcon
        )}
        {children}
        {!loading && rightIcon}
      </button>
    );
  },
);

Button.displayName = "Button";
export default Button;
