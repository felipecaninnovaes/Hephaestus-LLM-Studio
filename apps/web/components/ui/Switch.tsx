"use client";

import React, { type ReactNode, useId } from "react";

export interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: ReactNode;
  description?: ReactNode;
  id?: string;
  className?: string;
  size?: "sm" | "md";
  name?: string;
}

/**
 * Switch / Toggle canônico do Design System Arcane.
 * Implementa role="switch" semântico com foco acessível (WCAG 2.2 AA).
 */
export function Switch({
  checked,
  onCheckedChange,
  disabled = false,
  label,
  description,
  id: customId,
  className = "",
  size = "md",
  name,
}: SwitchProps) {
  const generatedId = useId();
  const id = customId || generatedId;

  const isSm = size === "sm";
  const trackWidth = isSm ? "w-7.5 h-4.5" : "w-10 h-6";
  const thumbSize = isSm ? "size-3.5" : "size-4.5";
  const translateActive = isSm ? "translate-x-3" : "translate-x-4.5";

  function handleChange(e: React.ChangeEvent<HTMLInputElement>) {
    if (disabled) return;
    onCheckedChange(e.target.checked);
  }

  return (
    <label
      htmlFor={id}
      className={`inline-flex items-center gap-3 select-none ${
        disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"
      } ${className}`.trim()}
    >
      <div className="relative inline-flex items-center">
        <input
          type="checkbox"
          role="switch"
          id={id}
          name={name}
          checked={checked}
          disabled={disabled}
          onChange={handleChange}
          aria-checked={checked}
          className="peer sr-only"
        />

        {/* Trilho do Switch */}
        <div
          aria-hidden="true"
          className={`${trackWidth} rounded-full border transition-colors p-0.5 peer-focus-visible:ring-2 peer-focus-visible:ring-brand-500 peer-focus-visible:ring-offset-2 peer-focus-visible:ring-offset-[var(--bg)] ${
            checked
              ? "border-brand-500 bg-brand-500 shadow-[0_0_14px_rgba(131,80,242,0.4)]"
              : "border-zinc-700 bg-zinc-900 hover:border-zinc-600"
          }`}
        >
          {/* Thumb deslizante */}
          <span
            className={`${thumbSize} block rounded-full bg-white shadow-md transform transition-transform duration-200 ease-[cubic-bezier(0.16,1,0.3,1)] ${
              checked ? translateActive : "translate-x-0"
            }`}
          />
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
