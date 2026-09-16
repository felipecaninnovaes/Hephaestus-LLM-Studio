"use client";

import { useMemo, useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { IconTrash } from "@/components/icons";
import { cleanupJobs } from "@/lib/jobs";
import { ApiError } from "@/lib/api";
import { showToast } from "@/components/studio/Toast";
import type { JobCleanupResponse } from "@/types/studio";

interface TerminalJobPreview {
  id: string;
  status: "done" | "failed" | "cancelled";
  createdAt: string;
  finishedAt?: string | null;
}

export interface JobCleanupDialogProps {
  open: boolean;
  onClose: () => void;
  /** jobs terminais já carregados, p/ pré-visualizar quantos casam (opcional, só informativo) */
  terminalJobs?: TerminalJobPreview[];
  onDone: (result: JobCleanupResponse) => void;
}

export function JobCleanupDialog({
  open,
  onClose,
  terminalJobs,
  onDone,
}: JobCleanupDialogProps) {
  const [days, setDays] = useState<string>("30");
  const [doneChecked, setDoneChecked] = useState(true);
  const [failedChecked, setFailedChecked] = useState(true);
  const [cancelledChecked, setCancelledChecked] = useState(true);
  const [busy, setBusy] = useState(false);

  const numericDays = useMemo(() => {
    const n = Number(days);
    return Number.isFinite(n) && n >= 0 ? n : null;
  }, [days]);

  const selectedStatuses = useMemo(() => {
    const s: ("done" | "failed" | "cancelled")[] = [];
    if (doneChecked) s.push("done");
    if (failedChecked) s.push("failed");
    if (cancelledChecked) s.push("cancelled");
    return s;
  }, [doneChecked, failedChecked, cancelledChecked]);

  const canConfirm = selectedStatuses.length > 0 || (numericDays !== null && numericDays > 0);

  // Preview: quantos jobs casam com os critérios atuais
  const matchCount = useMemo(() => {
    if (!terminalJobs || terminalJobs.length === 0) return null;
    const cutoff =
      numericDays !== null && numericDays > 0
        ? new Date(Date.now() - numericDays * 86400_000)
        : null;
    return terminalJobs.filter((j) => {
      if (!selectedStatuses.includes(j.status)) return false;
      if (cutoff) {
        const ref = j.finishedAt ?? j.createdAt;
        if (new Date(ref) >= cutoff) return false;
      }
      return true;
    }).length;
  }, [terminalJobs, numericDays, selectedStatuses]);

  async function handleConfirm() {
    setBusy(true);
    try {
      const result = await cleanupJobs({
        olderThanDays: numericDays !== null && numericDays > 0 ? numericDays : null,
        statuses: selectedStatuses.length > 0 ? selectedStatuses : null,
      });
      showToast(result.deleted > 0 ? `${result.deleted} job(s) removidos.` : "Nenhum job elegível para limpeza.", result.deleted > 0 ? "success" : "info");
      onDone(result);
      onClose();
    } catch (err) {
      if (err instanceof ApiError && err.code === "invalid_request") {
        showToast("Defina pelo menos um critério (dias ou status).", "error");
      } else {
        showToast("Falha ao limpar jobs.", "error");
      }
    } finally {
      setBusy(false);
    }
  }

  function handleClose() {
    if (busy) return;
    onClose();
  }

  return (
    <Modal
      open={open}
      onClose={handleClose}
      title="Limpar jobs antigos"
      icon={<IconTrash className="size-4" />}
      maxWidth="sm"
      busy={busy}
      ariaLabel="Limpar jobs antigos"
    >
      <div className="space-y-4">
        {/* Input de dias */}
        <div>
          <label htmlFor="job-cleanup-days" className="block text-2xs font-mono font-medium text-zinc-400 uppercase tracking-caps mb-1.5">
            Há mais de N dias
          </label>
          <input
            id="job-cleanup-days"
            type="number"
            min={0}
            value={days}
            onChange={(e) => setDays(e.target.value)}
            className="w-full rounded-lg border border-white/10 bg-white/[0.04] px-3 py-2 font-mono text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-brand-500/50 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)]"
            placeholder="30"
            disabled={busy}
          />
        </div>

        {/* Checkboxes de status */}
        <div>
          <span className="block text-2xs font-mono font-medium text-zinc-400 uppercase tracking-caps mb-2">
            Status
          </span>
          <div className="space-y-2">
            <label className="flex items-center gap-2.5 text-xs text-zinc-300 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={doneChecked}
                onChange={(e) => setDoneChecked(e.target.checked)}
                disabled={busy}
                className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
              />
              <span>Concluídos</span>
            </label>
            <label className="flex items-center gap-2.5 text-xs text-zinc-300 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={failedChecked}
                onChange={(e) => setFailedChecked(e.target.checked)}
                disabled={busy}
                className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
              />
              <span>Falhos</span>
            </label>
            <label className="flex items-center gap-2.5 text-xs text-zinc-300 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={cancelledChecked}
                onChange={(e) => setCancelledChecked(e.target.checked)}
                disabled={busy}
                className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-3.5"
              />
              <span>Cancelados</span>
            </label>
          </div>
        </div>

        {/* Linha de contexto */}
        {terminalJobs && terminalJobs.length > 0 && (
          <div className="rounded-lg border border-white/10 bg-white/[0.03] p-2.5 font-mono text-2xs text-zinc-400 space-y-0.5">
            <span>{terminalJobs.length} job(s) terminal(is) no histórico</span>
            {matchCount !== null && (
              <span className="block text-zinc-300">
                {matchCount} job(s) casam com os critérios atuais
              </span>
            )}
          </div>
        )}

        {/* Ações */}
        <div className="flex justify-end gap-2 pt-2 border-t border-white/10">
          <Button
            type="button"
            variant="ghost"
            size="md"
            onClick={handleClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="button"
            variant="destructive"
            size="lg"
            onClick={handleConfirm}
            loading={busy}
            disabled={!canConfirm || busy}
          >
            {busy ? "Aguarde…" : "Limpar jobs"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

export default JobCleanupDialog;
