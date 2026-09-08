"use client";

import React, { forwardRef, type SelectHTMLAttributes } from "react";
import { IconChevronDown } from "@/components/icons";

export interface SelectOption {
  value: string | number;
  label: string;
}

export interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  label?: string;
  error?: string | null;
  hint?: string;
  options?: SelectOption[];
  fontMono?: boolean;
}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(
  (
    {
      label,
      error,
      hint,
      options,
      fontMono = false,
      id,
      className = "",
      disabled,
      children,
      ...props
    },
    ref,
  ) => {
    const selectId = id || (label ? label.toLowerCase().replace(/\s+/g, "-") : undefined);

    return (
      <div className="w-full">
        {label && (
          <label
            htmlFor={selectId}
            className="tracking-caps mb-1.5 block font-mono text-[11px] font-medium uppercase text-zinc-300"
          >
            {label}
          </label>
        )}
        <div className="relative flex items-center">
          <select
            ref={ref}
            id={selectId}
            disabled={disabled}
            className={`w-full appearance-none rounded-xl border border-zinc-800 bg-black/40 text-zinc-100 py-2 pl-3 pr-9 text-[16px] sm:text-xs transition focus:border-brand-500 focus:outline-none focus-visible:ring-1 focus-visible:ring-brand-500/50 disabled:opacity-55 disabled:cursor-not-allowed ${
              fontMono ? "font-mono" : "font-sans"
            } ${error ? "border-rose-500/50 focus:border-rose-500" : ""} ${className}`.trim()}
            {...props}
          >
            {options
              ? options.map((opt) => (
                  <option
                    key={opt.value}
                    value={opt.value}
                    className="bg-zinc-900 text-zinc-100"
                  >
                    {opt.label}
                  </option>
                ))
              : children}
          </select>
          <span className="pointer-events-none absolute right-3 flex items-center text-zinc-500 [&_svg]:size-3.5">
            <IconChevronDown />
          </span>
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

Select.displayName = "Select";
export default Select;
