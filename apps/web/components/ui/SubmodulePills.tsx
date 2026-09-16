"use client";

import type React from "react";
import { useEffect, useRef } from "react";

export interface SubmodulePillItem<T extends string = string> {
  id: T;
  label: string;
  count?: number;
  icon?: React.ReactNode;
}

export interface SubmodulePillsProps<T extends string = string> {
  items: SubmodulePillItem<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
  size?: "sm" | "md";
}

export function SubmodulePills<T extends string = string>({
  items,
  value,
  onChange,
  className = "",
  size = "md",
}: SubmodulePillsProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    containerRef.current
      ?.querySelector('[data-active="true"]')
      ?.scrollIntoView({ inline: "nearest", block: "nearest" });
  }, [value]);

  const sizeClass =
    size === "sm"
      ? "h-7.5 px-2.5 text-xs"
      : "h-9 px-4 text-sm";

  return (
    <div className={`relative min-w-0 flex-1 sm:flex-none ${className}`.trim()}>
      <div
        ref={containerRef}
        role="tablist"
        className="no-scrollbar flex gap-1.5 overflow-x-auto py-0.5"
      >
        {items.map((item) => {
          const active = item.id === value;
          const labelWithCount =
            item.count != null ? `${item.label} ${item.count}` : item.label;

          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={active}
              data-active={active}
              title={labelWithCount}
              onClick={() => onChange(item.id)}
              className={`inline-flex shrink-0 items-center gap-1.5 truncate rounded-lg border font-medium whitespace-nowrap transition active:scale-[0.985] backdrop-blur-sm focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer ${sizeClass} ${
                active
                  ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                  : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
              }`}
            >
              {item.icon}
              <span className="truncate">{item.label}</span>
              {item.count != null && (
                <span className="font-mono text-2xs opacity-70">
                  {item.count}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export default SubmodulePills;
