"use client";

import React, { forwardRef, type InputHTMLAttributes } from "react";
import { IconSearch, IconX } from "@/components/icons";
import { Spinner } from "./Spinner";

export interface SearchInputProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  size?: "md" | "lg";
  loading?: boolean;
  onClear?: () => void;
}

export const SearchInput = forwardRef<HTMLInputElement, SearchInputProps>(
  (
    {
      size = "md",
      loading = false,
      value,
      onClear,
      placeholder = "Buscar…",
      className = "",
      disabled,
      ...props
    },
    ref,
  ) => {
    const hasValue = value != null && String(value).length > 0;
    const heightClass = size === "lg" ? "h-11 text-sm" : "h-9 text-xs";

    return (
      <div
        className={`relative flex items-center w-full rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm px-3 transition focus-within:border-brand-500/60 focus-within:ring-1 focus-within:ring-brand-500/30 ${heightClass} ${className}`.trim()}
      >
        {loading ? (
          <Spinner className="size-4 mr-2.5" />
        ) : (
          <IconSearch className="size-4 shrink-0 text-zinc-500 mr-2.5" />
        )}

        <input
          ref={ref}
          type="search"
          value={value}
          disabled={disabled}
          placeholder={placeholder}
          className="min-w-0 flex-1 bg-transparent text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none disabled:opacity-55 disabled:cursor-not-allowed"
          {...props}
        />

        {hasValue && onClear && !disabled && (
          <button
            type="button"
            onClick={onClear}
            aria-label="Limpar busca"
            className="ml-2 inline-flex size-5 items-center justify-center rounded-md text-zinc-400 hover:bg-white/[0.08] hover:text-white transition"
          >
            <IconX className="size-3.5" />
          </button>
        )}
      </div>
    );
  },
);

SearchInput.displayName = "SearchInput";
export default SearchInput;
