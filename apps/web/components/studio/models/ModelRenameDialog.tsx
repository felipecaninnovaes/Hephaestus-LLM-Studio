"use client";

import { IconX } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { GlassCard } from "@/components/ui/GlassCard";
import type { Model } from "@/types/studio";

export interface ModelRenameDialogProps {
  model: Model | null;
  name: string;
  setName: (val: string) => void;
  busy: boolean;
  onClose: () => void;
  onSubmit: (e: React.FormEvent) => void;
}

export function ModelRenameDialog({
  model,
  name,
  setName,
  busy,
  onClose,
  onSubmit,
}: ModelRenameDialogProps) {
  if (!model) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4">
      <GlassCard className="w-full max-w-md p-6 space-y-4 border border-zinc-700/60 bg-zinc-950/90 shadow-2xl">
        <div className="flex items-center justify-between">
          <h3 className="font-semibold text-zinc-100 text-base">
            Renomear Modelo
          </h3>
          <button
            type="button"
            onClick={onClose}
            className="text-zinc-400 hover:text-zinc-200 cursor-pointer"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>
        <p className="text-xs text-zinc-400">
          Altere o nome do arquivo para fácil identificação nos treinos, catálogo
          e downloads.
        </p>

        <form onSubmit={onSubmit} className="space-y-4">
          <div className="space-y-1.5">
            <label
              htmlFor="model-rename-input"
              className="text-2xs font-mono uppercase tracking-wider text-zinc-400"
            >
              Nome do arquivo
            </label>
            <input
              id="model-rename-input"
              type="text"
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
              className="w-full rounded-xl border border-zinc-700 bg-zinc-900 px-3 py-2 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500"
            />
          </div>

          <div className="flex justify-end gap-2 pt-2">
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={onClose}
              disabled={busy}
            >
              Cancelar
            </Button>
            <Button
              type="submit"
              variant="primary"
              size="sm"
              disabled={busy || !name.trim()}
              loading={busy}
            >
              Salvar
            </Button>
          </div>
        </form>
      </GlassCard>
    </div>
  );
}
