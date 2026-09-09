"use client";

import React, { useEffect, useState, type ReactNode } from "react";
import { IconX } from "@/components/icons";

export interface DrawerProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  description?: ReactNode;
  icon?: ReactNode;
  headerRight?: ReactNode;
  headerClassName?: string;
  children: ReactNode;
  side?: "right" | "left";
  widthClass?: string;
  busy?: boolean;
  ariaLabel?: string;
  className?: string;
  bodyClassName?: string;
  footer?: ReactNode;
  showCloseButton?: boolean;
}

export function Drawer({
  open,
  onClose,
  title,
  description,
  icon,
  headerRight,
  headerClassName = "",
  children,
  side = "right",
  widthClass = "w-full sm:w-[520px]",
  busy = false,
  ariaLabel,
  className = "",
  bodyClassName = "",
  footer,
  showCloseButton = true,
}: DrawerProps) {
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (open) {
      setMounted(true);
      const raf = requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          setVisible(true);
        });
      });
      return () => cancelAnimationFrame(raf);
    } else {
      setVisible(false);
      const timer = setTimeout(() => {
        setMounted(false);
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [open]);

  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  useEffect(() => {
    if (mounted) {
      const original = document.body.style.overflow;
      document.body.style.overflow = "hidden";
      return () => {
        document.body.style.overflow = original;
      };
    }
  }, [mounted]);

  if (!mounted) return null;

  const isRight = side === "right";
  const translateHidden = isRight ? "translate-x-full" : "-translate-x-full";
  const borderSide = isRight ? "border-l" : "border-r";
  const positionSide = isRight ? "right-0" : "left-0";
  const shadowClass = isRight
    ? "shadow-[-24px_0_60px_rgba(0,0,0,0.85)]"
    : "shadow-[24px_0_60px_rgba(0,0,0,0.85)]";

  return (
    <div className="fixed inset-0 z-50 overflow-hidden pointer-events-none">
      {/* Backdrop */}
      <div
        className={`fixed inset-0 bg-black/60 backdrop-blur-sm transition-opacity duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] pointer-events-auto ${
          visible ? "opacity-100" : "opacity-0"
        }`}
        onClick={() => {
          if (!busy) onClose();
        }}
        aria-hidden="true"
      />

      {/* Drawer Panel */}
      <aside
        role="dialog"
        aria-modal="true"
        aria-label={ariaLabel || title}
        className={`fixed inset-y-0 ${positionSide} flex flex-col ${borderSide} border-white/10 bg-[rgba(18,15,24,0.92)] text-zinc-100 ${shadowClass} backdrop-blur-2xl transition-transform duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] ${widthClass} pointer-events-auto ${
          visible ? "translate-x-0" : translateHidden
        } ${className}`.trim()}
      >
        {/* Hairline zenital com gradiente violeta no topo */}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute top-0 left-0 right-0 h-px"
          style={{
            background:
              "linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)",
          }}
        />

        {/* Header se título ou icon fornecido */}
        {(title || icon || showCloseButton || headerRight) && (
          <div
            className={`flex h-14 shrink-0 items-center justify-between border-b border-white/10 px-5 ${headerClassName}`.trim()}
          >
            <div className="flex items-center space-x-2.5 min-w-0 flex-1">
              {icon && (
                <div className="flex size-7 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400 [&_svg]:size-4">
                  {icon}
                </div>
              )}
              <div className="min-w-0 flex-1">
                {title && (
                  <h3 className="font-display text-sm font-bold text-white tracking-tight truncate">
                    {title}
                  </h3>
                )}
                {description && (
                  <div className="text-xs text-zinc-400 truncate">
                    {description}
                  </div>
                )}
              </div>
            </div>

            <div className="flex items-center space-x-1 shrink-0 ml-2">
              {headerRight}
              {showCloseButton && (
                <button
                  type="button"
                  onClick={onClose}
                  disabled={busy}
                  aria-label="Fechar painel"
                  className="inline-flex size-8 items-center justify-center rounded-lg border border-transparent bg-transparent text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 cursor-pointer disabled:opacity-50"
                >
                  <IconX className="size-4" />
                </button>
              )}
            </div>
          </div>
        )}

        {/* Corpo do Drawer */}
        <div className={`flex-1 min-h-0 overflow-y-auto ${bodyClassName}`.trim()}>
          {children}
        </div>

        {/* Rodapé fixo opcional */}
        {footer && (
          <div className="shrink-0 border-t border-white/10 bg-zinc-950/80 backdrop-blur-md">
            {footer}
          </div>
        )}
      </aside>
    </div>
  );
}

export default Drawer;
