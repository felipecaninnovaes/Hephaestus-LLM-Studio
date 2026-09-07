"use client";

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import type { Dataset } from "@/types/studio";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import { exportDataset, exportErrorMessage } from "@/lib/backup";
import {
  IconDownload,
  IconLayers,
  IconTarget,
  IconTrash,
} from "@/components/icons";

interface Props {
  dataset: Dataset;
  x: number;
  y: number;
  onClose: () => void;
  onDelete: (dataset: Dataset) => void;
}

export default function DatasetMenu({ dataset, x, y, onClose, onDelete }: Props) {
  const router = useRouter();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ top: y, left: x });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setPos({
      top: Math.max(8, Math.min(y, window.innerHeight - rect.height - 8)),
      left: Math.max(8, Math.min(x, window.innerWidth - rect.width - 8)),
    });
  }, [x, y]);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div
        className="fixed inset-0 z-40 bg-transparent"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        ref={ref}
        role="menu"
        aria-label={`Ações de ${dataset.title}`}
        className="glass-menu fixed z-50 min-w-[230px] rounded-2xl p-1.5 text-xs shadow-2xl"
        style={{ top: `${pos.top}px`, left: `${pos.left}px` }}
      >
        <div className="tracking-caps mb-1 border-b border-white/10 px-3 py-1 font-mono text-[10px] uppercase text-zinc-400">
          Ações de Contexto
        </div>
        <button
          type="button"
          role="menuitem"
          onClick={() => {
            onClose();
            router.push(`/datasets/${dataset.id}`);
          }}
          className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-zinc-200 transition-colors hover:bg-brand-500/20 hover:text-brand-300"
        >
          <IconLayers className="w-3.5 h-3.5 shrink-0" />
          <span>Abrir galeria</span>
        </button>
        <button
          type="button"
          role="menuitem"
          disabled
          title="Treino chega na fatia 4"
          className="flex w-full cursor-not-allowed items-center gap-2 rounded-xl px-3 py-2 text-left text-zinc-500"
        >
          <IconTarget className="w-3.5 h-3.5 shrink-0" />
          <span>Treinar neste dataset</span>
        </button>
        <button
          type="button"
          role="menuitem"
          onClick={async () => {
            onClose();
            try {
              await exportDataset(dataset.id, dataset.slug);
            } catch (err) {
              if (
                err instanceof ApiError &&
                (err.code === "unauthorized" || err.status === 401)
              ) {
                router.replace("/login");
                return;
              }
              showToast(
                err instanceof ApiError
                  ? exportErrorMessage(err.code)
                  : "Falha ao exportar dataset.",
                "error",
              );
            }
          }}
          className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-zinc-200 transition-colors hover:bg-brand-500/20 hover:text-brand-300"
        >
          <IconDownload className="w-3.5 h-3.5 shrink-0" />
          <span>Exportar</span>
        </button>
        <div className="my-1 h-px bg-white/10" />
        <button
          type="button"
          role="menuitem"
          onClick={() => {
            onClose();
            onDelete(dataset);
          }}
          className="flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left text-rose-300 transition-colors hover:bg-rose-500/20 hover:text-rose-200"
        >
          <IconTrash className="w-3.5 h-3.5 shrink-0" />
          <span>Excluir</span>
        </button>
      </div>
    </>
  );
}
