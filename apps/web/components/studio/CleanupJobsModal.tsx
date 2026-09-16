"use client";

import { useState } from "react";
import { IconTrash } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Select, type SelectOption } from "@/components/ui/Select";
import { ApiError } from "@/lib/api";
import { cleanupJobs } from "@/lib/jobs";
import type { JobCleanupResponse } from "@/types/studio";
import { showToast } from "./Toast";

type AgeValue = "7" | "30" | "90" | "all";

const AGE_OPTIONS: SelectOption<AgeValue>[] = [
  { value: "7", label: "Há mais de 7 dias" },
  { value: "30", label: "Há mais de 30 dias" },
  { value: "90", label: "Há mais de 90 dias" },
  { value: "all", label: "Todos os jobs terminais" },
];

interface Props {
  open: boolean;
  onClose: () => void;
  onSuccess: (res: JobCleanupResponse) => void;
}

export function CleanupJobsModal({ open, onClose, onSuccess }: Props) {
  const [age, setAge] = useState<AgeValue>("30");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const res = await cleanupJobs({
        olderThanDays: age === "all" ? null : Number(age),
      });
      if (res.deleted === 0) {
        showToast("Nenhum job elegível para limpeza.", "info");
      } else {
        showToast(`${res.deleted} job(s) removidos.`, "success");
      }
      onSuccess(res);
      onClose();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message || "Falha ao limpar jobs.");
      } else {
        setError("Falha ao limpar jobs.");
      }
      showToast("Falha ao limpar jobs.", "error");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onClose}
      title="Limpar jobs antigos"
      description="Remove jobs terminais em lote."
      icon={<IconTrash className="size-4" />}
      maxWidth="sm"
      busy={busy}
    >
      <form onSubmit={handleSubmit} className="space-y-5">
        <div className="space-y-1.5">
          <label className="text-xs font-medium text-zinc-300">
            Idade dos jobs
          </label>
          <Select
            value={age}
            onChange={(val) => {
              setAge(val);
              setError(null);
            }}
            options={AGE_OPTIONS}
          />
        </div>

        <div className="rounded-xl border border-white/10 bg-zinc-950/60 p-3 text-xs text-zinc-400 leading-relaxed">
          <p>
            Somente jobs terminais (concluídos, falhos ou cancelados) são
            afetados. Os artefatos e pesos derivados no catálogo são removidos,
            mas a galeria de gerações é preservada.
          </p>
        </div>

        {error && (
          <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-xs text-rose-300 font-medium">
            {error}
          </div>
        )}

        <div className="flex items-center justify-end gap-3 pt-2 border-t border-white/10">
          <Button
            type="button"
            variant="secondary"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="submit"
            variant="destructive"
            disabled={busy}
            loading={busy}
          >
            {busy ? (
              "Limpando…"
            ) : (
              <>
                <IconTrash className="size-3.5 mr-1.5" />
                <span>Limpar jobs</span>
              </>
            )}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

export default CleanupJobsModal;
