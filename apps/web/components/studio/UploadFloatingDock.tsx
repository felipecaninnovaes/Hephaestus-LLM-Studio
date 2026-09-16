"use client";

import React from "react";
import { IconUpload, IconCheck, IconAlertTriangle, IconX } from "@/components/icons";
import type { UploadResultItem } from "@/types/studio";

export interface UploadFloatingDockProps {
  uploading: boolean;
  uploadSent: number;
  uploadCount: number;
  uploadBatchInfo: { batchIndex: number; batchCount: number } | null;
  lastResults: UploadResultItem[] | null;
  onCancel: () => void;
  onOpenAudit: () => void;
  onDismiss: () => void;
}

export function UploadFloatingDock({
  uploading,
  uploadSent,
  uploadCount,
  uploadBatchInfo,
  lastResults,
  onCancel,
  onOpenAudit,
  onDismiss,
}: UploadFloatingDockProps) {
  if (!uploading && !lastResults) return null;

  const percent = uploadCount > 0 ? Math.min(100, Math.round((uploadSent / uploadCount) * 100)) : 0;

  const storedCount = lastResults?.filter((r) => r.status === "stored").length ?? 0;
  const duplicateCount = lastResults?.filter((r) => r.status === "duplicate").length ?? 0;
  const rejectedCount =
    lastResults?.filter((r) => r.status === "rejected" || r.status === "failed").length ?? 0;

  return (
    <section
      aria-label="Status do upload em andamento"
      className="fixed bottom-5 right-5 z-40 w-80 sm:w-96 rounded-2xl border border-white/10 bg-zinc-950/90 p-3.5 shadow-2xl backdrop-blur-xl transition-all duration-200 animate-in fade-in slide-in-from-bottom-4"
    >
      {/* Hairline zenital */}
      <div
        className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-brand-400/50 to-transparent"
        aria-hidden="true"
      />

      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center space-x-2.5 min-w-0">
          <div
            className={`flex size-8 shrink-0 items-center justify-center rounded-lg border ${
              uploading
                ? "border-brand-500/40 bg-brand-500/20 text-brand-300 animate-pulse"
                : rejectedCount > 0
                ? "border-status-alert/40 bg-status-alert/20 text-amber-300"
                : "border-status-success/40 bg-status-success/20 text-[#a7f3d0]"
            }`}
          >
            {uploading ? (
              <IconUpload className="size-4" />
            ) : rejectedCount > 0 ? (
              <IconAlertTriangle className="size-4" />
            ) : (
              <IconCheck className="size-4" />
            )}
          </div>
          <div className="min-w-0">
            <p className="truncate text-xs font-semibold text-white">
              {uploading
                ? `Enviando amostras… (${percent}%)`
                : rejectedCount > 0
                ? "Upload concluído com avisos"
                : "Upload concluído com sucesso"}
            </p>
            <p className="font-mono text-3xs text-zinc-400">
              {uploading ? (
                uploadBatchInfo ? (
                  <>
                    {uploadSent}/{uploadCount} imgs · Lote {uploadBatchInfo.batchIndex}/
                    {uploadBatchInfo.batchCount}
                  </>
                ) : (
                  <>{uploadSent}/{uploadCount} imgs processadas</>
                )
              ) : (
                <>
                  <span className="text-status-success font-medium">{storedCount} salvas</span>
                  {duplicateCount > 0 && ` · ${duplicateCount} duplicadas`}
                  {rejectedCount > 0 && ` · ${rejectedCount} rejeitadas`}
                </>
              )}
            </p>
          </div>
        </div>

        <div className="flex items-center space-x-1 shrink-0">
          {uploading ? (
            <button
              type="button"
              onClick={onCancel}
              title="Cancelar upload restante"
              className="rounded-lg border border-rose-500/40 bg-rose-500/10 px-2 py-1 font-mono text-3xs font-semibold text-rose-300 transition-colors hover:bg-rose-500/20 cursor-pointer"
            >
              Cancelar
            </button>
          ) : (
            <>
              {lastResults && lastResults.length > 0 && (
                <button
                  type="button"
                  onClick={onOpenAudit}
                  title="Abrir relatório detalhado do upload"
                  className="rounded-lg border border-brand-500/30 bg-brand-500/15 px-2 py-1 font-mono text-3xs font-semibold text-brand-300 transition-colors hover:bg-brand-500/25 cursor-pointer"
                >
                  Relatório
                </button>
              )}
              <button
                type="button"
                onClick={onDismiss}
                aria-label="Fechar notificação"
                className="rounded-lg p-1 text-zinc-400 hover:text-white hover:bg-white/5 cursor-pointer transition-colors"
              >
                <IconX className="size-3.5" />
              </button>
            </>
          )}
        </div>
      </div>

      {/* Barra de Progresso durante upload */}
      {uploading && (
        <div className="mt-2.5 h-1.5 w-full overflow-hidden rounded-full bg-zinc-800/80">
          <div
            className="h-full bg-gradient-to-r from-brand-500 to-status-success transition-all duration-300 ease-out"
            style={{ width: `${percent}%` }}
          />
        </div>
      )}
    </section>
  );
}

export default UploadFloatingDock;
