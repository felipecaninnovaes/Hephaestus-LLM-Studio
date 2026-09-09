"use client";

import type { ReactNode } from "react";
import { Modal } from "./Modal";
import { Button } from "./Button";

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  busy: boolean;
  onConfirm: () => void;
  onClose: () => void;
}

export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger,
  busy,
  onConfirm,
  onClose,
}: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      maxWidth="sm"
      busy={busy}
    >
      <div className="text-xs leading-relaxed text-zinc-300">{body}</div>
      <div className="mt-5 flex justify-end gap-2">
        <Button
          type="button"
          variant="ghost"
          size="md"
          onClick={onClose}
          disabled={busy}
        >
          Cancelar
        </Button>
        <Button
          type="button"
          variant={danger ? "destructive" : "primary"}
          size="lg"
          onClick={onConfirm}
          loading={busy}
        >
          {busy ? "Aguarde…" : confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}

export default ConfirmDialog;
