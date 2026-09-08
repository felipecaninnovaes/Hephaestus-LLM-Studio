"use client";

import React, { forwardRef, type InputHTMLAttributes } from "react";

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string | null;
  hint?: string;
  fontMono?: boolean;
  prefixIcon?: React.ReactNode;
  suffixIcon?: React.ReactNode;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      label,
      error,
      hint,
      fontMono = false,
      prefixIcon,
      suffixIcon,
      id,
      className = "",
      disabled,
      ...props
    },
    ref,
  ) => {
    const inputId = id || (label ? label.toLowerCase().replace(/\s+/g, "-") : undefined);

    const hasPrefix = Boolean(prefixIcon);
    const hasSuffix = Boolean(suffixIcon);

    return (
      <div className="w-full">
        {label && (
          <label
            htmlFor={inputId}
            className="tracking-caps mb-1.5 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            {label}
          </label>
        )}
        <div className="relative flex items-center">
          {hasPrefix && (
            <span className="pointer-events-none absolute left-3 flex items-center text-zinc-500 [&_svg]:size-4">
              {prefixIcon}
            </span>
          )}
          <input
            ref={ref}
            id={inputId}
            disabled={disabled}
            className={`w-full rounded-xl border border-zinc-800 bg-black/40 text-zinc-100 placeholder:text-zinc-500 transition focus:border-brand-500 focus:outline-none focus-visible:ring-1 focus-visible:ring-brand-500/50 disabled:opacity-55 disabled:cursor-not-allowed ${
              fontMono ? "font-mono" : "font-sans"
            } ${hasPrefix ? "pl-9" : "pl-3"} ${hasSuffix ? "pr-9" : "pr-3"} py-2 text-[16px] sm:text-xs ${
              error ? "border-rose-500/50 focus:border-rose-500" : ""
            } ${className}`.trim()}
            {...props}
          />
          {hasSuffix && (
            <span className="pointer-events-none absolute right-3 flex items-center text-zinc-500 [&_svg]:size-4">
              {suffixIcon}
            </span>
          )}
        </div>
        {error && (
          <p role="alert" className="mt-1 font-mono text-[11px] text-rose-300">
            {error}
          </p>
        )}
        {!error && hint && (
          <p className="mt-1 font-mono text-[11px] text-zinc-500 leading-normal">
            {hint}
          </p>
        )}
      </div>
    );
  },
);

Input.displayName = "Input";
export default Input;
