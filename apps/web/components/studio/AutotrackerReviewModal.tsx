"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { IconCheck, IconTarget, IconSparkles } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { ApiError } from "@/lib/api";
import { applyAutotrackerBoxes, getAutotrackerPreview } from "@/lib/autotracker";
import type { AutotrackerPreviewResponse, Job } from "@/types/studio";
import { showToast } from "./Toast";

interface Props {
  open: boolean;
  job: Job;
  onClose: () => void;
  onApplied?: () => void;
}

export function AutotrackerReviewModal({
  open,
  job,
  onClose,
  onApplied,
}: Props) {
  const router = useRouter();
  const [loading, setLoading] = useState(true);
  const [preview, setPreview] = useState<AutotrackerPreviewResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedMissing, setSelectedMissing] = useState<Set<string>>(new Set());
  const [overwrite, setOverwrite] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setError(null);

    getAutotrackerPreview(job.id)
      .then((data) => {
        if (!active) return;
        setPreview(data);
        // Por padrão, já vem marcadas todas as classes ausentes para facilitar aceitação imediata
        setSelectedMissing(new Set(data.missingClasses.map((c) => c.name)));
        setLoading(false);
      })
      .catch((err) => {
        if (!active) return;
        if (err instanceof ApiError) {
          setError(err.message || "Falha ao carregar prévia do AutoTracker.");
        } else {
          setError("Erro inesperado ao buscar dados do artefato.");
        }
        setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [open, job.id]);

  function toggleMissingClass(name: string) {
    setSelectedMissing((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  }

  function handleSelectAllMissing() {
    if (!preview) return;
    if (selectedMissing.size === preview.missingClasses.length) {
      setSelectedMissing(new Set());
    } else {
      setSelectedMissing(new Set(preview.missingClasses.map((c) => c.name)));
    }
  }

  async function handleApply() {
    setBusy(true);
    setError(null);
    try {
      const createList = Array.from(selectedMissing);
      const res = await applyAutotrackerBoxes(job.id, {
        overwrite,
        createMissingClasses: createList.length > 0 ? createList : undefined,
      });

      showToast(
        `${res.applied} boxes aplicadas em ${res.images} imagens (${res.skipped} ignoradas).`,
        "success",
        job.datasetId
          ? {
              label: "Abrir dataset",
              onClick: () => {
                onClose();
                router.push(`/datasets/${job.datasetId}`);
              },
            }
          : undefined,
      );
      onApplied?.();
      onClose();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message || "Falha ao aplicar anotações ao dataset.");
      } else {
        setError("Erro ao aplicar detecções do AutoTracker.");
      }
    } finally {
      setBusy(false);
    }
  }

  const allMissingSelected =
    preview &&
    preview.missingClasses.length > 0 &&
    selectedMissing.size === preview.missingClasses.length;

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onClose}
      title="Revisar e Aplicar AutoTracker"
      description={`Job ${job.id.slice(0, 8)} • Modelo: ${job.model || "World/YOLO"}`}
      maxWidth="lg"
    >
      <div className="space-y-5">
        {loading ? (
          <div className="flex flex-col items-center justify-center py-12 space-y-3">
            <div className="size-8 rounded-full border-2 border-brand-500/30 border-t-brand-400 animate-spin" />
            <p className="font-mono text-xs text-zinc-400">
              Analisando detecções e classes no artefato…
            </p>
          </div>
        ) : error && !preview ? (
          <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-xs text-rose-300">
            <p className="font-semibold mb-1">Erro ao carregar prévia</p>
            <p>{error}</p>
          </div>
        ) : preview ? (
          <>
            {/* Estatísticas resumidas */}
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2.5">
              <div className="rounded-xl border border-white/10 bg-zinc-900/60 p-3">
                <span className="text-[11px] text-zinc-400 block font-mono">Imagens</span>
                <span className="text-lg font-bold font-mono text-zinc-100">
                  {preview.totalImages}
                </span>
              </div>
              <div className="rounded-xl border border-white/10 bg-zinc-900/60 p-3">
                <span className="text-[11px] text-zinc-400 block font-mono">Total de Boxes</span>
                <span className="text-lg font-bold font-mono text-brand-400">
                  {preview.totalBoxes}
                </span>
              </div>
              <div className="rounded-xl border border-white/10 bg-zinc-900/60 p-3">
                <span className="text-[11px] text-zinc-400 block font-mono">Classes Existentes</span>
                <span className="text-lg font-bold font-mono text-zinc-100">
                  {preview.existingClasses.length}
                </span>
              </div>
              <div className="rounded-xl border border-white/10 bg-zinc-900/60 p-3">
                <span className="text-[11px] text-zinc-400 block font-mono">Classes Ausentes</span>
                <span className={`text-lg font-bold font-mono ${preview.missingClasses.length > 0 ? "text-amber-400" : "text-zinc-500"}`}>
                  {preview.missingClasses.length}
                </span>
              </div>
            </div>

            {/* Seção de Classes Ausentes */}
            {preview.missingClasses.length > 0 ? (
              <div className="rounded-xl border border-amber-500/30 bg-amber-500/5 p-4 space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <IconSparkles className="size-4 text-amber-400" />
                    <h4 className="text-xs font-semibold text-amber-300">
                      Classes ausentes detectadas pelo AutoTracker
                    </h4>
                  </div>
                  <button
                    type="button"
                    onClick={handleSelectAllMissing}
                    className="text-[11px] font-mono text-amber-300 hover:text-amber-200 transition-colors cursor-pointer"
                  >
                    {allMissingSelected ? "Desmarcar todas" : "Selecionar todas"}
                  </button>
                </div>
                <p className="text-xs text-zinc-400 leading-relaxed">
                  As classes abaixo foram detectadas nas imagens, mas ainda não constam no dataset. Marque as que você deseja criar automaticamente para aceitar suas anotações:
                </p>

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 max-h-48 overflow-y-auto pr-1">
                  {preview.missingClasses.map((item) => {
                    const isChecked = selectedMissing.has(item.name);
                    return (
                      <label
                        key={item.name}
                        onClick={() => toggleMissingClass(item.name)}
                        className={`flex items-center justify-between p-2.5 rounded-lg border transition-all cursor-pointer ${
                          isChecked
                            ? "border-amber-500/40 bg-amber-500/15 text-zinc-100"
                            : "border-white/10 bg-zinc-900/40 text-zinc-400 hover:border-white/20"
                        }`}
                      >
                        <div className="flex items-center space-x-2.5 min-w-0">
                          <div
                            className={`size-4 rounded flex items-center justify-center border transition-colors ${
                              isChecked
                                ? "border-amber-400 bg-amber-400 text-zinc-950"
                                : "border-zinc-600 bg-zinc-800"
                            }`}
                          >
                            {isChecked && <IconCheck className="size-3 stroke-[3]" />}
                          </div>
                          <span className="text-xs font-mono truncate font-medium">
                            {item.name}
                          </span>
                        </div>
                        <span className="font-mono text-[11px] px-2 py-0.5 rounded-md border border-white/10 bg-zinc-900/60 text-zinc-300 shrink-0 ml-2">
                          {item.boxesCount} boxes
                        </span>
                      </label>
                    );
                  })}
                </div>
              </div>
            ) : (
              <div className="rounded-xl border border-white/10 bg-zinc-900/40 p-3.5 flex items-center space-x-2.5 text-xs text-zinc-300">
                <IconCheck className="size-4 text-brand-400 shrink-0" />
                <span>
                  Todas as classes detectadas já existem cadastradas no dataset.
                </span>
              </div>
            )}

            {/* Resumo de Classes Existentes */}
            {preview.existingClasses.length > 0 && (
              <div className="space-y-1.5">
                <span className="text-[11px] font-mono text-zinc-400 uppercase tracking-wider block">
                  Classes já existentes no dataset ({preview.existingClasses.length})
                </span>
                <div className="flex flex-wrap gap-1.5 max-h-28 overflow-y-auto">
                  {preview.existingClasses.map((item) => (
                    <span
                      key={item.name}
                      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg border border-white/10 bg-zinc-900/60 font-mono text-xs text-zinc-300"
                    >
                      <span>{item.name}</span>
                      <span className="text-[10px] text-brand-400">({item.boxesCount})</span>
                    </span>
                  ))}
                </div>
              </div>
            )}

            {/* Opções de Aplicação */}
            <div className="rounded-xl border border-white/10 bg-zinc-900/40 p-3.5 space-y-2">
              <label
                onClick={() => setOverwrite(!overwrite)}
                className="flex items-start space-x-2.5 cursor-pointer"
              >
                <div
                  className={`mt-0.5 size-4 rounded flex items-center justify-center border transition-colors ${
                    overwrite
                      ? "border-brand-500 bg-brand-500 text-zinc-950"
                      : "border-zinc-600 bg-zinc-800"
                  }`}
                >
                  {overwrite && <IconCheck className="size-3 stroke-[3]" />}
                </div>
                <div className="text-xs">
                  <span className="font-semibold text-zinc-200 block">
                    Sobrescrever todas as anotações da imagem (overwrite)
                  </span>
                  <span className="text-zinc-400 text-[11px] leading-relaxed">
                    Se desmarcado, preserva anotações manuais e substitui apenas as de origem AutoTracker.
                  </span>
                </div>
              </label>
            </div>

            {error && (
              <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-xs text-rose-300 font-medium">
                {error}
              </div>
            )}
          </>
        ) : null}

        {/* Ações do Rodapé */}
        <div className="flex items-center justify-end gap-3 pt-3 border-t border-white/10">
          <Button
            type="button"
            variant="secondary"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="button"
            variant="primary"
            onClick={handleApply}
            disabled={busy || loading || !preview}
          >
            {busy ? (
              "Aplicando boxes…"
            ) : (
              <>
                <IconTarget className="size-3.5 mr-1.5" />
                <span>
                  Confirmar e Aplicar{" "}
                  {selectedMissing.size > 0 && `(+${selectedMissing.size} classes)`}
                </span>
              </>
            )}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

export default AutotrackerReviewModal;
