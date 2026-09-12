"use client";

import React, { useState } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { IconCheck, IconAlertTriangle, IconTrash, IconCopy } from "@/components/icons";
import type { UploadResultItem } from "@/types/studio";

export interface UploadAuditModalProps {
  open: boolean;
  onClose: () => void;
  results: UploadResultItem[];
}

export function UploadAuditModal({
  open,
  onClose,
  results,
}: UploadAuditModalProps) {
  const [tab, setTab] = useState<"stored" | "duplicate" | "rejected">("stored");
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  const stored = results.filter((r) => r.status === "stored");
  const duplicate = results.filter((r) => r.status === "duplicate");
  const rejected = results.filter((r) => r.status === "rejected" || r.status === "failed");

  function copyText(text: string, key: string) {
    navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 1500);
  }

  function formatBytes(bytes?: number | null) {
    if (!bytes || bytes <= 0) return "—";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  const currentList = tab === "stored" ? stored : tab === "duplicate" ? duplicate : rejected;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Relatório de Auditoria do Upload"
      description="Inspeção detalhada de todas as amostras processadas nesta remessa."
      maxWidth="xl"
    >
      <div className="flex flex-col gap-4">
        {/* Pílulas de Navegação por Categoria */}
        <div className="flex flex-wrap items-center gap-1.5 border-b border-zinc-800/80 pb-3">
          <button
            type="button"
            onClick={() => setTab("stored")}
            className={`flex items-center space-x-1.5 rounded-lg px-3 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
              tab === "stored"
                ? "border border-[#34d399]/40 bg-[#34d399]/15 text-[#a7f3d0]"
                : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
            }`}
          >
            <IconCheck className="size-3.5 text-[#34d399]" />
            <span>Armazenadas</span>
            <span className="rounded bg-black/40 px-1.5 py-0.2 text-[10px] text-zinc-300">
              {stored.length}
            </span>
          </button>

          <button
            type="button"
            onClick={() => setTab("duplicate")}
            className={`flex items-center space-x-1.5 rounded-lg px-3 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
              tab === "duplicate"
                ? "border border-amber-500/40 bg-amber-500/15 text-amber-300"
                : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
            }`}
          >
            <IconAlertTriangle className="size-3.5 text-amber-400" />
            <span>Duplicadas</span>
            <span className="rounded bg-black/40 px-1.5 py-0.2 text-[10px] text-zinc-300">
              {duplicate.length}
            </span>
          </button>

          <button
            type="button"
            onClick={() => setTab("rejected")}
            className={`flex items-center space-x-1.5 rounded-lg px-3 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
              tab === "rejected"
                ? "border border-rose-500/40 bg-rose-500/15 text-rose-300"
                : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
            }`}
          >
            <IconTrash className="size-3.5 text-rose-400" />
            <span>Rejeitadas / Falhas</span>
            <span className="rounded bg-black/40 px-1.5 py-0.2 text-[10px] text-zinc-300">
              {rejected.length}
            </span>
          </button>
        </div>

        {/* Lista de Itens */}
        <div className="max-h-80 overflow-y-auto rounded-xl border border-zinc-800/80 bg-zinc-950/40 p-2 space-y-1.5">
          {currentList.length === 0 ? (
            <div className="py-8 text-center font-mono text-xs text-zinc-500">
              Nenhum item nesta categoria.
            </div>
          ) : (
            currentList.map((item, idx) => (
              <div
                key={`${item.filename}-${idx}`}
                className="flex items-center justify-between gap-3 rounded-lg border border-white/[0.04] bg-white/[0.02] px-3 py-2 text-xs transition-colors hover:bg-white/[0.04]"
              >
                <div className="flex items-center space-x-2.5 min-w-0">
                  <span className="font-mono text-[10px] text-zinc-500 w-5 text-right">
                    #{idx + 1}
                  </span>
                  <div className="min-w-0">
                    <p
                      title={item.filename}
                      className="truncate font-mono text-xs font-medium text-zinc-200"
                    >
                      {item.filename}
                    </p>
                    <p className="font-mono text-[10px] text-zinc-400">
                      {tab === "stored" ? (
                        <>
                          {item.width && item.height ? `${item.width}×${item.height}px · ` : ""}
                          {formatBytes(item.bytes)} · Normalizado WebP
                        </>
                      ) : tab === "duplicate" ? (
                        <span className="text-amber-400/90">
                          Identificado mesmo hash MD5 — imagem já existente no dataset
                        </span>
                      ) : (
                        <span className="text-rose-400/90">
                          Motivo: {item.reason === "unsupported_media" ? "Formato de arquivo incompatível" : item.reason === "too_large" ? "Excede o limite máximo (200MB)" : item.reason ?? item.status}
                        </span>
                      )}
                    </p>
                  </div>
                </div>

                <div className="flex items-center space-x-2 shrink-0">
                  <button
                    type="button"
                    onClick={() => copyText(item.filename, `${item.filename}-${idx}`)}
                    title="Copiar nome/hash do arquivo"
                    className="flex items-center space-x-1 rounded px-2 py-1 font-mono text-[10px] text-zinc-400 hover:text-zinc-200 hover:bg-white/5 transition-colors cursor-pointer"
                  >
                    <IconCopy className="size-3" />
                    <span>{copiedKey === `${item.filename}-${idx}` ? "Copiado!" : "Copiar"}</span>
                  </button>
                </div>
              </div>
            ))
          )}
        </div>

        {/* Rodapé Informativo */}
        <div className="flex items-center justify-between border-t border-zinc-800/80 pt-3">
          <p className="font-mono text-[11px] text-zinc-400">
            Total nesta remessa: <span className="text-zinc-200 font-semibold">{results.length}</span> arquivos
          </p>
          <Button type="button" variant="secondary" size="sm" onClick={onClose}>
            Fechar
          </Button>
        </div>
      </div>
    </Modal>
  );
}

export default UploadAuditModal;
