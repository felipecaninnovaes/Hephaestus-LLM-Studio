"use client";

import React, { useState, useEffect, useMemo } from "react";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Spinner } from "@/components/ui/Spinner";
import {
  IconCheck,
  IconX,
  IconSparkles,
  IconSearch,
  IconRefresh,
  IconAlertTriangle,
  IconInfo,
  IconImage,
} from "@/components/icons";
import { getAutolabelPreview, applyAutolabelCaptions } from "@/lib/autolabel";
import { autolabelErrorMessage, type AutolabelPreviewItem } from "@/types/studio";
import { showToast } from "@/components/ui/Toast";

export interface AutolabelReviewModalProps {
  open: boolean;
  onClose: () => void;
  jobId: string | null;
  datasetId?: string | null;
  onApplied?: () => void;
}

interface ReviewItemState extends AutolabelPreviewItem {
  editedCaption: string;
  selected: boolean;
}

export function AutolabelReviewModal({
  open,
  onClose,
  jobId,
  datasetId,
  onApplied,
}: AutolabelReviewModalProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<ReviewItemState[]>([]);
  const [modelName, setModelName] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [filterTab, setFilterTab] = useState<"all" | "selected" | "modified" | "existing">("all");
  const [overwrite, setOverwrite] = useState(false);
  const [applying, setApplying] = useState(false);

  // Carregar dados da prévia ao abrir
  useEffect(() => {
    if (!open || !jobId) return;

    let active = true;
    setLoading(true);
    setError(null);

    getAutolabelPreview(jobId)
      .then((data) => {
        if (!active) return;
        setModelName(data.model ?? null);
        const mapped: ReviewItemState[] = data.items.map((item) => ({
          ...item,
          editedCaption: item.generatedCaption,
          selected: true,
        }));
        setItems(mapped);
      })
      .catch((err: unknown) => {
        if (!active) return;
        const code = (err as { code?: string })?.code || "unknown";
        setError(autolabelErrorMessage(code) || "Falha ao carregar prévia das legendas.");
      })
      .finally(() => {
        if (active) setLoading(false);
      });

    return () => {
      active = false;
    };
  }, [open, jobId]);

  // Contadores
  const stats = useMemo(() => {
    const total = items.length;
    const selected = items.filter((i) => i.selected).length;
    const modified = items.filter((i) => i.editedCaption !== i.generatedCaption).length;
    const existing = items.filter((i) => !!i.currentCaption).length;
    return { total, selected, modified, existing };
  }, [items]);

  // Filtro e busca
  const filteredItems = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    return items.filter((item) => {
      // Filtro por tab
      if (filterTab === "selected" && !item.selected) return false;
      if (filterTab === "modified" && item.editedCaption === item.generatedCaption) return false;
      if (filterTab === "existing" && !item.currentCaption) return false;

      // Filtro por busca
      if (query) {
        const matchesFilename = item.filename.toLowerCase().includes(query);
        const matchesGenerated = item.generatedCaption.toLowerCase().includes(query);
        const matchesEdited = item.editedCaption.toLowerCase().includes(query);
        const matchesCurrent = item.currentCaption?.toLowerCase().includes(query) ?? false;
        return matchesFilename || matchesGenerated || matchesEdited || matchesCurrent;
      }

      return true;
    });
  }, [items, filterTab, searchQuery]);

  // Manipuladores de itens
  function toggleItemSelection(filename: string) {
    setItems((prev) =>
      prev.map((it) => (it.filename === filename ? { ...it, selected: !it.selected } : it)),
    );
  }

  function handleCaptionChange(filename: string, newCaption: string) {
    setItems((prev) =>
      prev.map((it) => (it.filename === filename ? { ...it, editedCaption: newCaption } : it)),
    );
  }

  function handleResetCaption(filename: string) {
    setItems((prev) =>
      prev.map((it) =>
        it.filename === filename ? { ...it, editedCaption: it.generatedCaption } : it,
      ),
    );
  }

  function handleSelectAll() {
    setItems((prev) => prev.map((it) => ({ ...it, selected: true })));
  }

  function handleDeselectAll() {
    setItems((prev) => prev.map((it) => ({ ...it, selected: false })));
  }

  function handleResetAllModifications() {
    setItems((prev) => prev.map((it) => ({ ...it, editedCaption: it.generatedCaption })));
  }

  // Ação de aplicar
  async function handleApply() {
    if (!jobId) return;
    const selectedItems = items.filter((i) => i.selected);
    if (selectedItems.length === 0) {
      showToast("Selecione ao menos uma imagem para aplicar legendas.", "info");
      return;
    }

    setApplying(true);
    try {
      const payloadItems = selectedItems.map((i) => ({
        filename: i.filename,
        caption: i.editedCaption.trim(),
      }));

      const res = await applyAutolabelCaptions(jobId, {
        datasetId: datasetId ?? undefined,
        overwrite,
        items: payloadItems,
      });

      showToast(
        `${res.applied} legendas aplicadas (${res.skipped} ignoradas) em ${res.images} imagens!`,
        "success",
      );
      onApplied?.();
      onClose();
    } catch (err: unknown) {
      const code = (err as { code?: string })?.code || "unknown";
      showToast(autolabelErrorMessage(code) || "Falha ao aplicar legendas curadas.", "error");
    } finally {
      setApplying(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Revisão e Curadoria de Legendas"
      description={
        modelName
          ? `Inspecione, edite e aprove as legendas geradas pelo modelo ${modelName} antes de aplicar ao dataset.`
          : "Inspecione, edite e aprove as legendas geradas antes de aplicar ao dataset."
      }
      maxWidth="xl"
      className="max-h-[90vh] flex flex-col"
    >
      <div className="flex flex-col gap-4 min-h-[400px]">
        {/* Loading State */}
        {loading && (
          <div className="flex flex-col items-center justify-center p-12 text-center text-xs font-mono text-zinc-400 space-y-3">
            <Spinner className="size-6" />
            <span>Carregando amostras geradas do artefato captions.jsonl…</span>
          </div>
        )}

        {/* Error State */}
        {!loading && error && (
          <div className="flex flex-col items-center justify-center p-8 text-center space-y-3 rounded-xl border border-rose-500/30 bg-rose-500/10 text-rose-300">
            <IconAlertTriangle className="size-8 text-rose-400" />
            <p className="text-xs font-medium">{error}</p>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => {
                if (!jobId) return;
                setLoading(true);
                setError(null);
                getAutolabelPreview(jobId)
                  .then((data) => {
                    setModelName(data.model ?? null);
                    setItems(
                      data.items.map((i) => ({
                        ...i,
                        editedCaption: i.generatedCaption,
                        selected: true,
                      })),
                    );
                  })
                  .catch((e: unknown) => {
                    const code = (e as { code?: string })?.code || "unknown";
                    setError(autolabelErrorMessage(code));
                  })
                  .finally(() => setLoading(false));
              }}
            >
              <IconRefresh className="size-3.5" />
              <span>Tentar Novamente</span>
            </Button>
          </div>
        )}

        {/* Loaded Content */}
        {!loading && !error && (
          <>
            {/* Header com Estatísticas e Filtros */}
            <div className="space-y-3">
              {/* Barra de Pílulas de Estatísticas */}
              <div className="flex flex-wrap items-center gap-1.5 border-b border-zinc-800/80 pb-3">
                <button
                  type="button"
                  onClick={() => setFilterTab("all")}
                  className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
                    filterTab === "all"
                      ? "border border-brand-500/40 bg-brand-500/15 text-brand-300"
                      : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
                  }`}
                >
                  <IconSparkles className="size-3.5 text-brand-400" />
                  <span>Total Geradas</span>
                  <span className="rounded bg-black/40 px-1.5 py-0.2 text-3xs text-zinc-300">
                    {stats.total}
                  </span>
                </button>

                <button
                  type="button"
                  onClick={() => setFilterTab("selected")}
                  className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
                    filterTab === "selected"
                      ? "border border-status-success/40 bg-status-success/15 text-[#a7f3d0]"
                      : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
                  }`}
                >
                  <IconCheck className="size-3.5 text-status-success" />
                  <span>Selecionadas</span>
                  <span className="rounded bg-black/40 px-1.5 py-0.2 text-3xs text-zinc-300">
                    {stats.selected} / {stats.total}
                  </span>
                </button>

                <button
                  type="button"
                  onClick={() => setFilterTab("modified")}
                  className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
                    filterTab === "modified"
                      ? "border border-status-alert/40 bg-status-alert/15 text-amber-300"
                      : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
                  }`}
                >
                  <IconInfo className="size-3.5 text-amber-400" />
                  <span>Editadas</span>
                  <span className="rounded bg-black/40 px-1.5 py-0.2 text-3xs text-zinc-300">
                    {stats.modified}
                  </span>
                </button>

                {stats.existing > 0 && (
                  <button
                    type="button"
                    onClick={() => setFilterTab("existing")}
                    className={`flex items-center space-x-1.5 rounded-lg px-2.5 py-1.5 font-mono text-xs font-medium transition-colors cursor-pointer ${
                      filterTab === "existing"
                        ? "border border-blue-500/40 bg-blue-500/15 text-blue-300"
                        : "border border-transparent text-zinc-400 hover:bg-white/5 hover:text-zinc-200"
                    }`}
                  >
                    <IconImage className="size-3.5 text-blue-400" />
                    <span>Possui Legenda Atual</span>
                    <span className="rounded bg-black/40 px-1.5 py-0.2 text-3xs text-zinc-300">
                      {stats.existing}
                    </span>
                  </button>
                )}
              </div>

              {/* Toolbar: Busca + Ações em Massa */}
              <div className="flex flex-col sm:flex-row items-stretch sm:items-center justify-between gap-2.5">
                <div className="relative flex-1">
                  <IconSearch className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-zinc-400" />
                  <input
                    type="text"
                    placeholder="Filtrar por nome de arquivo ou legenda…"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    className="w-full rounded-lg border border-white/10 bg-black/40 py-1.5 pl-8 pr-3 text-xs text-zinc-200 placeholder:text-zinc-500 focus:border-brand-500 focus:outline-none focus:ring-1 focus:ring-brand-500 font-mono"
                  />
                  {searchQuery && (
                    <button
                      type="button"
                      onClick={() => setSearchQuery("")}
                      className="absolute right-2 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-zinc-200 cursor-pointer"
                    >
                      <IconX className="size-3.5" />
                    </button>
                  )}
                </div>

                <div className="flex items-center gap-1.5 shrink-0">
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={handleSelectAll}
                    title="Marcar todas as imagens para aplicação"
                  >
                    <span>Marcar Todas</span>
                  </Button>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={handleDeselectAll}
                    title="Desmarcar todas as imagens"
                  >
                    <span>Desmarcar Todas</span>
                  </Button>
                  {stats.modified > 0 && (
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      onClick={handleResetAllModifications}
                      title="Restaurar legendas originais geradas pelo VLM"
                    >
                      <span>Desfazer Edições</span>
                    </Button>
                  )}
                </div>
              </div>
            </div>

            {/* Lista de Imagens & Legendas com Scroll */}
            <div className="max-h-[50vh] overflow-y-auto space-y-3.5 pr-1.5 [scrollbar-width:thin]">
              {filteredItems.length === 0 ? (
                <div className="flex flex-col items-center justify-center p-8 text-center text-xs font-mono text-zinc-400 rounded-xl border border-white/5 bg-white/[0.01]">
                  <span>Nenhuma imagem encontrada com o filtro atual.</span>
                </div>
              ) : (
                filteredItems.map((item) => {
                  const isModified = item.editedCaption !== item.generatedCaption;
                  const charCount = item.editedCaption.length;

                  return (
                    <div
                      key={item.filename}
                      className={`relative flex flex-col md:flex-row gap-3.5 rounded-xl border p-3 transition-all ${
                        item.selected
                          ? "border-white/10 bg-white/[0.02] hover:border-brand-500/30 hover:bg-white/[0.04]"
                          : "border-white/5 bg-black/20 opacity-60 hover:opacity-80"
                      }`}
                    >
                      {/* Coluna Visual: Checkbox + Thumbnail + Filename */}
                      <div className="flex md:flex-col items-start gap-2.5 w-full md:w-44 shrink-0">
                        <div className="flex items-center gap-2 w-full">
                          <input
                            type="checkbox"
                            checked={item.selected}
                            onChange={() => toggleItemSelection(item.filename)}
                            className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-4 cursor-pointer"
                          />
                          <span
                            className="font-mono text-2xs font-semibold text-zinc-200 truncate flex-1"
                            title={item.filename}
                          >
                            {item.filename}
                          </span>
                        </div>

                        {/* Thumbnail */}
                        <div className="relative aspect-square w-20 md:w-full rounded-lg overflow-hidden border border-white/10 bg-black/60 shrink-0">
                          {item.imageUrl ? (
                            /* eslint-disable-next-line @next/next/no-img-element */
                            <img
                              src={item.imageUrl}
                              alt={item.filename}
                              className="h-full w-full object-cover object-center"
                              loading="lazy"
                            />
                          ) : (
                            <div className="flex h-full w-full items-center justify-center text-zinc-600">
                              <IconImage className="size-6" />
                            </div>
                          )}
                        </div>
                      </div>

                      {/* Coluna Conteúdo: Comparador e Editor Inline */}
                      <div className="flex-1 flex flex-col gap-2 min-w-0">
                        {/* Legenda Atual no Banco (se houver) */}
                        {item.currentCaption && (
                          <div className="rounded-lg border border-blue-500/20 bg-blue-500/[0.04] p-2 text-2xs font-mono space-y-1">
                            <div className="flex items-center justify-between text-3xs text-blue-300">
                              <span className="font-semibold uppercase tracking-wider">
                                Legenda Atual no Dataset
                              </span>
                              <span className="rounded bg-blue-500/20 px-1.5 py-0.5 text-blue-200">
                                origem: {item.currentOrigin || "manual"}
                              </span>
                            </div>
                            <p className="text-zinc-300 italic text-2xs line-clamp-2">
                              &ldquo;{item.currentCaption}&rdquo;
                            </p>
                          </div>
                        )}

                        {/* Editor de Legenda Gerada */}
                        <div className="space-y-1 flex-1 flex flex-col">
                          <div className="flex items-center justify-between text-2xs font-mono">
                            <span className="flex items-center gap-1.5 text-zinc-300 font-semibold">
                              <IconSparkles className="size-3 text-brand-400" />
                              Legenda Gerada pelo VLM
                              {isModified && (
                                <span className="rounded bg-status-alert/20 text-amber-300 text-3xs px-1.5 py-0.2">
                                  Editada
                                </span>
                              )}
                            </span>
                            <div className="flex items-center gap-2">
                              {isModified && (
                                <button
                                  type="button"
                                  onClick={() => handleResetCaption(item.filename)}
                                  className="text-3xs text-zinc-400 hover:text-brand-300 underline cursor-pointer"
                                >
                                  Restaurar original
                                </button>
                              )}
                              <span
                                className={`text-3xs ${
                                  charCount > 8000 ? "text-rose-400 font-bold" : "text-zinc-500"
                                }`}
                              >
                                {charCount} / 8000
                              </span>
                            </div>
                          </div>

                          <textarea
                            value={item.editedCaption}
                            onChange={(e) => handleCaptionChange(item.filename, e.target.value)}
                            disabled={!item.selected}
                            rows={3}
                            placeholder="Texto da legenda…"
                            className="w-full rounded-lg border border-white/10 bg-black/40 p-2.5 text-xs text-zinc-100 placeholder:text-zinc-600 focus:border-brand-500 focus:outline-none focus:ring-1 focus:ring-brand-500 disabled:opacity-50 disabled:cursor-not-allowed resize-y font-mono leading-relaxed"
                          />
                        </div>
                      </div>
                    </div>
                  );
                })
              )}
            </div>

            {/* Rodapé do Modal */}
            <div className="flex flex-col sm:flex-row items-stretch sm:items-center justify-between gap-3 border-t border-white/10 pt-3">
              <label className="flex items-center gap-2 text-xs text-zinc-300 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={overwrite}
                  onChange={(e) => setOverwrite(e.target.checked)}
                  className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40 size-4"
                />
                <span>Sobrescrever legendas existentes (inclusive manuais e importadas)</span>
              </label>

              <div className="flex items-center justify-end gap-2 shrink-0">
                <Button type="button" variant="secondary" size="sm" onClick={onClose} disabled={applying}>
                  <span>Cancelar</span>
                </Button>
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  disabled={applying || stats.selected === 0}
                  loading={applying}
                  onClick={handleApply}
                >
                  <IconCheck className="size-3.5 text-brand-400" />
                  <span>
                    {applying
                      ? "Aplicando…"
                      : `Aplicar ${stats.selected} ${
                          stats.selected === 1 ? "Legenda" : "Legendas"
                        } ao Dataset`}
                  </span>
                </Button>
              </div>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}
