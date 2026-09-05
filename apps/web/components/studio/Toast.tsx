"use client";

import { useSyncExternalStore } from "react";

export type ToastType = "success" | "error" | "info";

interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
}

const MAX_VISIBLE = 3;
const DISMISS_MS = 3600;

let toasts: ToastItem[] = [];
let seq = 0;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ToastItem[] {
  return toasts;
}

export function showToast(
  message: string,
  type: ToastType = "info",
): void {
  const id = ++seq;
  toasts = [...toasts, { id, message, type }].slice(-MAX_VISIBLE);
  emit();
  setTimeout(() => {
    toasts = toasts.filter((toast) => toast.id !== id);
    emit();
  }, DISMISS_MS);
}

const DOT_BY_TYPE: Record<ToastType, string> = {
  success: "bg-emerald-400",
  error: "bg-rose-400",
  info: "bg-cyan-400",
};

export function ToastHost() {
  const items = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  if (items.length === 0) return null;
  return (
    <div className="fixed bottom-5 right-5 z-50 flex flex-col gap-2 items-end max-w-[calc(100vw-2.5rem)]">
      {items.map((toast) => (
        <div
          key={toast.id}
          role="status"
          aria-live="polite"
          className="rounded-xl glass-menu px-4 py-3 text-xs text-zinc-200 flex items-center space-x-2.5 shadow-2xl animate-toast-in"
        >
          <span
            aria-hidden="true"
            className={`w-2 h-2 rounded-full shrink-0 ${DOT_BY_TYPE[toast.type]}`}
          ></span>
          <span>{toast.message}</span>
        </div>
      ))}
    </div>
  );
}
