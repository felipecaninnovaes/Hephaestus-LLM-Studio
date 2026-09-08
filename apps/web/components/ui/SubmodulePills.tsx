"use client";

import React, { useEffect, useRef } from "react";

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
}

export function SubmodulePills<T extends string = string>({
  items,
  value,
  onChange,
  className = "",
}: SubmodulePillsProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    containerRef.current
      ?.querySelector('[data-active="true"]')
      ?.scrollIntoView({ inline: "nearest", block: "nearest" });
  }, [value]);

  return (
    <div className={`relative min-w-0 flex-1 sm:flex-none ${className}`.trim()}>
      <div
        ref={containerRef}
        role="tablist"
        className="no-scrollbar flex gap-1.5 overflow-x-auto py-0.5 pr-8"
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
              className={`inline-flex h-9 shrink-0 items-center gap-1.5 truncate rounded-lg border px-4 text-sm font-medium whitespace-nowrap transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer ${
                active
                  ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                  : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
              }`}
            >
              {item.icon}
              <span className="truncate">{item.label}</span>
              {item.count != null && (
                <span className="font-mono text-[11px] opacity-70">
                  {item.count}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {/* Fade edge à direita para indicar overflow de scroll */}
      <div
        className="pointer-events-none absolute top-0 right-0 h-full w-8 bg-gradient-to-l from-zinc-950 to-transparent"
        aria-hidden="true"
      />
    </div>
  );
}

export default SubmodulePills;
