"use client";

import { useEffect } from "react";
import type { ReactNode } from "react";
import { IconX } from "@/components/icons";

interface Props {
  open: boolean;
  title: string;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  busy: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

export default function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger,
  busy,
  onConfirm,
  onClose,
}: Props) {
  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="glass-modal relative w-full max-w-sm rounded-2xl p-6 text-zinc-100 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-bold text-white">{title}</h3>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar diálogo"
            className="rounded-lg p-1 text-zinc-400 transition-colors hover:text-white"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>
        <div className="mt-3 text-xs leading-relaxed text-zinc-300">{body}</div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-xl bg-zinc-900 px-4 py-2 text-xs font-medium text-zinc-300 hover:bg-zinc-800"
          >
            Cancelar
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={busy}
            className={
              danger
                ? "rounded-xl bg-rose-500/90 px-5 py-2 text-xs font-semibold text-zinc-50 hover:bg-rose-500 disabled:opacity-60"
                : "rounded-xl bg-emerald-500 px-5 py-2 text-xs font-semibold text-zinc-950 disabled:opacity-60"
            }
          >
            {busy ? "Aguarde…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
