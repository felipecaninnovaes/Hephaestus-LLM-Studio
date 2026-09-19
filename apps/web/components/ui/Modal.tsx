"use client";

import React, { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { IconX } from "@/components/icons";
import { useFocusTrap } from "@/hooks/useFocusTrap";
import { useBodyScrollLock } from "@/hooks/useBodyScrollLock";
import { usePortalRoot } from "@/hooks/usePortalRoot";
export interface ModalProps {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  icon?: React.ReactNode;
  children: ReactNode;
  maxWidth?: "sm" | "md" | "lg" | "xl";
  busy?: boolean;
  role?: "dialog" | "alertdialog";
  ariaLabel?: string;
  className?: string;
  bodyClassName?: string;
  headerRight?: React.ReactNode;
  showCloseButton?: boolean;
  onDragOver?: React.DragEventHandler<HTMLDivElement>;
  onDragLeave?: React.DragEventHandler<HTMLDivElement>;
  onDrop?: React.DragEventHandler<HTMLDivElement>;
}

const MAX_WIDTH_CLASSES = {
  sm: "max-w-sm",
  md: "max-w-md",
  lg: "max-w-lg",
  xl: "max-w-xl",
};

export function Modal({
  open,
  onClose,
  title,
  description,
  icon,
  children,
  maxWidth = "lg",
  busy = false,
  role = "dialog",
  ariaLabel,
  className = "",
  bodyClassName = "",
  headerRight,
  showCloseButton = true,
  onDragOver,
  onDragLeave,
  onDrop,
}: ModalProps) {
  const portalRoot = usePortalRoot();
  const containerRef = useFocusTrap<HTMLDivElement>(open && !busy);
  useBodyScrollLock(open);

  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open || !portalRoot) return null;

  return createPortal(
    // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: backdrop suplementar — o fechamento por teclado é global (Escape) e há botão fechar explícito; o backdrop fica fora da tab-order de propósito.
    <div
      className="fixed inset-0 z-modal flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        ref={containerRef}
        role={role}
        aria-modal="true"
        aria-label={ariaLabel || title}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
        className={`glass-modal relative w-full ${MAX_WIDTH_CLASSES[maxWidth]} rounded-2xl p-6 text-zinc-100 shadow-2xl overflow-hidden max-h-[90vh] flex flex-col ${className}`.trim()}
      >
        {/* Hairline zenital com gradiente violeta no topo */}
        <span
          aria-hidden="true"
          className="pointer-events-none absolute top-0 left-6 right-6 h-px"
          style={{
            background:
              "linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)",
          }}
        />

        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-white/10 pb-4">
          <div className="flex items-center space-x-2.5 min-w-0">
            {icon && (
              <div className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 backdrop-blur-sm text-brand-400 [&_svg]:size-4">
                {icon}
              </div>
            )}
            <div className="min-w-0">
              <h3 className="font-display text-sm font-bold text-white truncate">
                {title}
              </h3>
              {description && (
                <div className="text-xs text-zinc-400 truncate">
                  {description}
                </div>
              )}
            </div>
          </div>
          <div className="flex items-center space-x-1.5 shrink-0">
            {headerRight}
            {showCloseButton && (
              <button
                type="button"
                onClick={onClose}
                disabled={busy}
                aria-label="Fechar diálogo"
                className="rounded-lg border border-transparent bg-transparent p-1 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer"
              >
                <IconX className="size-4" />
              </button>
            )}
          </div>
        </div>

        {/* Conteúdo com scroll interno se necessário */}
        <div className={`mt-4 flex-1 overflow-y-auto ${bodyClassName}`.trim()}>{children}</div>
      </div>
    </div>
  , portalRoot);
}

export default Modal;
