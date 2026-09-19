"use client";

import React, { type ReactNode, useId } from "react";
import { IconCheck } from "@/components/icons";

export interface CheckboxProps {
  checked: boolean | "indeterminate";
  onCheckedChange?: (checked: boolean) => void;
  disabled?: boolean;
  label?: ReactNode;
  description?: ReactNode;
  id?: string;
  className?: string;
  error?: boolean;
  name?: string;
  value?: string;
}

/**
 * Checkbox canônico do Design System Arcane.
 * Baseado em input semântico com overlay customizado de alta fidelidade visual (WCAG 2.2 AA).
 */
export function Checkbox({
  checked,
  onCheckedChange,
  disabled = false,
  label,
  description,
  id: customId,
  className = "",
  error = false,
  name,
  value,
}: CheckboxProps) {
  const generatedId = useId();
  const id = customId || generatedId;
  const isChecked = checked === true;
  const isIndeterminate = checked === "indeterminate";

  function handleChange(e: React.ChangeEvent<HTMLInputElement>) {
    if (disabled || !onCheckedChange) return;
    onCheckedChange(e.target.checked);
  }

  return (
    <label
      htmlFor={id}
      className={`inline-flex items-start gap-2.5 select-none ${
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"
      } ${className}`.trim()}
    >
      <div className="relative flex items-center justify-center pt-0.5">
        <input
          type="checkbox"
          id={id}
          name={name}
          value={value}
          checked={isChecked}
          disabled={disabled}
          onChange={handleChange}
          className="peer sr-only"
        />

        <div
          aria-hidden="true"
          className={`size-4.5 rounded-md border transition-all flex items-center justify-center peer-focus-visible:ring-2 peer-focus-visible:ring-brand-500 peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-[var(--bg)] ${
            error
              ? "border-status-danger bg-status-danger/10"
              : isChecked || isIndeterminate
                ? "border-brand-500 bg-brand-500 text-white shadow-[0_0_12px_rgba(131,80,242,0.35)]"
                : "border-zinc-700 bg-zinc-900/90 text-transparent hover:border-zinc-500 hover:bg-zinc-800"
          }`.trim()}
        >
          {isChecked && <IconCheck className="size-3 stroke-[2.5]" />}
          {isIndeterminate && (
            <span className="h-0.5 w-2.5 rounded-full bg-white" />
          )}
        </div>
      </div>

      {(label || description) && (
        <div className="flex flex-col text-left">
          {label && (
            <span
              className={`text-xs font-medium ${
                disabled ? "cursor-not-allowed text-zinc-500" : "text-zinc-200"
              }`}
            >
              {label}
            </span>
          )}
          {description && (
            <span className="text-2xs text-zinc-400 mt-0.5 leading-relaxed">
              {description}
            </span>
          )}
        </div>
      )}
    </label>
  );
}
