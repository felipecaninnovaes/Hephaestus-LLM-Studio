"use client";

import React, { useEffect, useState, useCallback } from "react";
import { useRouter } from "next/navigation";
import { getImage, putCaption } from "@/lib/images";
import { copyToClipboard } from "@/lib/clipboard";
import { Button } from "@/components/ui/Button";
import {
  IconX,
  IconChevronRight,
  IconTrash,
  IconBoxSelect,
  IconCopy,
  IconTarget,
  IconSparkles,
  IconCheck,
} from "@/components/icons";
import { showToast } from "@/components/studio/Toast";
import type { ImageItem, ImageDetail, Dataset, StudioClass } from "@/types/studio";

export interface ImageQuickLookModalProps {
  open: boolean;
  onClose: () => void;
  dataset: Dataset | null;
  items: ImageItem[];
  currentIndex: number;
  onNavigate: (index: number) => void;
  onDelete?: (item: ImageItem) => void;
  onEditImage?: (item: ImageItem) => void;
  onCaptionUpdated?: (imageId: string, caption: string) => void;
}

export function ImageQuickLookModal({
  open,
  onClose,
  dataset,
  items,
  currentIndex,
  onNavigate,
  onDelete,
  onEditImage,
  onCaptionUpdated,
}: ImageQuickLookModalProps) {
  const router = useRouter();
  const currentItem = items[currentIndex];

  const [detail, setDetail] = useState<ImageDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);
  const [isEditingCaption, setIsEditingCaption] = useState(false);
  const [captionInput, setCaptionInput] = useState("");
  const [savingCaption, setSavingCaption] = useState(false);

  const fetchDetail = useCallback(async (item: ImageItem) => {
    if (!dataset) return;
    setLoading(true);
    try {
      const d = await getImage(dataset.id, item.id);
      setDetail(d);
      setCaptionInput(d.caption?.text ?? "");
      setIsEditingCaption(false);
    } catch {
      setDetail(null);
      setCaptionInput("");
      setIsEditingCaption(false);
    } finally {
      setLoading(false);
    }
  }, [dataset]);

  useEffect(() => {
    if (open && currentItem) {
      fetchDetail(currentItem);
    } else {
      setDetail(null);
      setIsEditingCaption(false);
    }
  }, [open, currentItem, fetchDetail]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!open) return;
      if (e.key === "Escape" || e.code === "Space") {
        e.preventDefault();
        onClose();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        if (currentIndex > 0) onNavigate(currentIndex - 1);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        if (currentIndex < items.length - 1) onNavigate(currentIndex + 1);
      } else if (e.key === "Enter" || e.key.toLowerCase() === "e") {
        if (currentItem && dataset && dataset.category === "yolo") {
          e.preventDefault();
          if (onEditImage) {
            onEditImage(currentItem);
          } else {
            router.push(`/datasets/${dataset.id}/annotate/${currentItem.id}`);
          }
        }
      }
    },
    [open, onClose, currentIndex, items.length, onNavigate, currentItem, dataset, router]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!open || !currentItem) return null;

  function copyText(text: string) {
    void copyToClipboard(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }

  async function handleSaveCaption() {
    if (!dataset || !currentItem) return;
    const trimmed = captionInput.trim();
    if (!trimmed) {
      showToast("A legenda não pode ser vazia.", "info");
      return;
    }
    if (trimmed.length > 8000) {
      showToast("A legenda não pode exceder 8000 caracteres.", "info");
      return;
    }
    setSavingCaption(true);
    try {
      const saved = await putCaption(dataset.id, currentItem.id, {
        text: trimmed,
        origin: "manual",
      });
      setDetail((prev) => (prev ? { ...prev, caption: saved } : prev));
      currentItem.caption = saved.text;
      onCaptionUpdated?.(currentItem.id, saved.text);
      setIsEditingCaption(false);
      showToast("Legenda salva com sucesso!", "success");
    } catch {
      showToast("Falha ao salvar legenda.", "error");
    } finally {
      setSavingCaption(false);
    }
  }

  function formatBytes(bytes?: number | null) {
    if (!bytes || bytes <= 0) return "—";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  const boxes = detail?.boxes ?? [];
  const classesMap = new Map<string, StudioClass>((dataset?.classes ?? []).map((c: StudioClass) => [c.id, c]));

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Inspeção rápida de ${currentItem.filename}`}
      className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-6 backdrop-blur-md bg-black/80 animate-in fade-in duration-150"
    >
      <div className="relative flex flex-col md:flex-row h-[90vh] w-full max-w-6xl overflow-hidden rounded-2xl border border-white/10 bg-zinc-950/95 shadow-2xl backdrop-blur-2xl">
        {/* Hairline zenital */}
        <div
          className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-brand-400/60 to-transparent"
          aria-hidden="true"
        />

        {/* Área Visual Principal */}
        <div className="relative flex flex-1 flex-col items-center justify-center overflow-hidden bg-black/50 p-4 select-none">
          {/* Top Bar Sobre a Imagem */}
          <div className="absolute top-3 inset-x-3 z-10 flex items-center justify-between pointer-events-none">
            <span className="rounded-md border border-white/10 bg-black/60 px-2 py-0.5 font-mono text-2xs text-zinc-300 backdrop-blur-sm pointer-events-auto">
              {currentIndex + 1} / {items.length}
            </span>
            <div className="flex items-center space-x-1.5 pointer-events-auto">
              <span className="hidden sm:inline-flex items-center px-2 py-0.5 rounded border border-white/10 bg-black/60 font-mono text-3xs text-zinc-400 backdrop-blur-sm">
                Espaço para fechar · ← → navegar {dataset?.category === "yolo" ? "· E editar bbox" : ""}
              </span>
              <button
                type="button"
                onClick={onClose}
                aria-label="Fechar inspeção rápida"
                className="rounded-lg border border-white/10 bg-black/60 p-1.5 text-zinc-300 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
              >
                <IconX className="size-4" />
              </button>
            </div>
          </div>

          {/* Canvas da Imagem com Bounding Boxes Sobrepostas */}
          <div className="relative max-h-[75vh] max-w-full flex items-center justify-center overflow-hidden">
            <img
              src={currentItem.url}
              alt={currentItem.filename}
              className="max-h-[75vh] max-w-full object-contain rounded-lg shadow-lg border border-zinc-800/80"
            />

            {/* Overlay de Bounding Boxes */}
            {boxes.length > 0 && (
              <svg
                aria-hidden="true"
                focusable="false"
                className="absolute inset-0 size-full pointer-events-none"
                viewBox={`0 0 ${detail?.width ?? currentItem.width ?? 100} ${
                  detail?.height ?? currentItem.height ?? 100
                }`}
                preserveAspectRatio="none"
              >
                {boxes.map((box) => {
                  const cls = classesMap.get(box.classId);
                  const color = cls?.color ?? "#8350f2";
                  return (
                    <g key={box.id}>
                      <rect
                        x={box.x}
                        y={box.y}
                        width={box.w}
                        height={box.h}
                        fill={`${color}22`}
                        stroke={color}
                        strokeWidth="2"
                        vectorEffect="non-scaling-stroke"
                      />
                      <text
                        x={box.x + 3}
                        y={box.y > 15 ? box.y - 4 : box.y + 12}
                        fill="#ffffff"
                        fontSize="11"
                        fontFamily="monospace"
                        fontWeight="600"
                        style={{ filter: "drop-shadow(0 1px 2px rgba(0,0,0,0.8))" }}
                      >
                        {cls?.name ?? "tag"}
                      </text>
                    </g>
                  );
                })}
              </svg>
            )}
          </div>

          {/* Botões de Navegação Anterior / Próximo */}
          {currentIndex > 0 && (
            <button
              type="button"
              onClick={() => onNavigate(currentIndex - 1)}
              aria-label="Imagem anterior"
              className="absolute left-3 top-1/2 -translate-y-1/2 rounded-full border border-white/10 bg-black/70 p-2 text-zinc-300 hover:text-white hover:bg-black/90 transition-all cursor-pointer backdrop-blur-sm"
            >
              <svg aria-hidden="true" focusable="false" className="size-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <polyline points="15 18 9 12 15 6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          )}
          {currentIndex < items.length - 1 && (
            <button
              type="button"
              onClick={() => onNavigate(currentIndex + 1)}
              aria-label="Próxima imagem"
              className="absolute right-3 top-1/2 -translate-y-1/2 rounded-full border border-white/10 bg-black/70 p-2 text-zinc-300 hover:text-white hover:bg-black/90 transition-all cursor-pointer backdrop-blur-sm"
            >
              <svg aria-hidden="true" focusable="false" className="size-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <polyline points="9 18 15 12 9 6" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          )}
        </div>

        {/* Sidebar Técnica de Metadados e Labels */}
        <div className="w-full md:w-88 flex flex-col justify-between border-t md:border-t-0 md:border-l border-zinc-800/80 bg-zinc-950 p-4 sm:p-5 overflow-y-auto [scrollbar-width:thin]">
          <div className="space-y-4">
            <div>
              <span className="rounded border border-white/15 bg-zinc-900 px-2 py-0.5 font-mono text-3xs font-semibold uppercase tracking-caps text-zinc-300">
                Split {currentItem.split}
              </span>
              <h3
                title={currentItem.filename}
                className="mt-2 truncate font-mono text-sm font-semibold text-white"
              >
                {currentItem.filename}
              </h3>
            </div>

            {/* Painel de Metadados Técnicos */}
            <div className="rounded-xl border border-zinc-800/80 bg-zinc-900/50 p-3 space-y-2 text-xs font-mono">
              <div className="flex items-center justify-between text-zinc-400">
                <span>Resolução:</span>
                <span className="text-zinc-200">
                  {currentItem.width && currentItem.height
                    ? `${currentItem.width} × ${currentItem.height} px`
                    : "—"}
                </span>
              </div>
              <div className="flex items-center justify-between text-zinc-400">
                <span>Tamanho:</span>
                <span className="text-zinc-200">{formatBytes(currentItem.bytes)}</span>
              </div>
              <div className="flex items-center justify-between text-zinc-400">
                <span>Formato:</span>
                <span className="text-brand-300 uppercase">
                  {currentItem.mediaType ?? "webp"}
                </span>
              </div>
              {dataset?.category === "yolo" && (
                <div className="flex items-center justify-between text-zinc-400">
                  <span>Bounding Boxes:</span>
                  <span className="text-status-success font-medium">
                    {boxes.length} {boxes.length === 1 ? "box" : "boxes"}
                  </span>
                </div>
              )}
            </div>

            {/* Bloco de Legenda / Caption (Preview e Edição) */}
            <div className="space-y-2 rounded-xl border border-white/10 bg-zinc-900/40 p-3">
              <div className="flex items-center justify-between text-2xs font-mono">
                <span className="flex items-center gap-1.5 font-semibold text-zinc-300 uppercase tracking-caps">
                  <IconSparkles className="size-3.5 text-brand-400" />
                  Legenda / Caption
                </span>
                {detail?.caption && (
                  <div className="flex items-center gap-1">
                    <span
                      className={`rounded px-1.5 py-0.2 text-3xs uppercase font-semibold ${
                        detail.caption.origin === "autolabel"
                          ? "border border-status-success/40 bg-status-success/15 text-[#a7f3d0]"
                          : detail.caption.origin === "manual"
                          ? "border border-purple-500/40 bg-purple-500/15 text-purple-300"
                          : "border border-blue-500/40 bg-blue-500/15 text-blue-300"
                      }`}
                    >
                      {detail.caption.origin}
                    </span>
                    {detail.caption.model && (
                      <span
                        className="rounded border border-white/10 bg-black/40 px-1.5 py-0.2 text-3xs text-zinc-400 truncate max-w-[100px]"
                        title={detail.caption.model}
                      >
                        {detail.caption.model}
                      </span>
                    )}
                  </div>
                )}
              </div>

              {loading ? (
                <p className="font-mono text-xs text-zinc-500">Carregando legenda…</p>
              ) : isEditingCaption ? (
                <div className="space-y-2">
                  <textarea
                    value={captionInput}
                    onChange={(e) => setCaptionInput(e.target.value)}
                    rows={4}
                    placeholder="Digite a legenda desta imagem…"
                    className="w-full rounded-lg border border-white/15 bg-black/60 p-2 text-xs text-zinc-100 placeholder:text-zinc-500 focus:border-brand-500 focus:outline-none focus:ring-1 focus:ring-brand-500 font-mono leading-relaxed resize-y"
                    // biome-ignore lint/a11y/noAutofocus: textarea de modo de edição ativado por clique em Editar dentro de modal; foco acompanha a transição para edição
                    autoFocus
                  />
                  <div className="flex items-center justify-between">
                    <span
                      className={`font-mono text-3xs ${
                        captionInput.length > 8000 ? "text-rose-400 font-bold" : "text-zinc-500"
                      }`}
                    >
                      {captionInput.length} / 8000
                    </span>
                    <div className="flex items-center gap-1.5">
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        disabled={savingCaption}
                        onClick={() => {
                          setCaptionInput(detail?.caption?.text ?? "");
                          setIsEditingCaption(false);
                        }}
                      >
                        <span>Cancelar</span>
                      </Button>
                      <Button
                        type="button"
                        variant="primary"
                        size="sm"
                        disabled={savingCaption || !captionInput.trim()}
                        loading={savingCaption}
                        onClick={handleSaveCaption}
                      >
                        <IconCheck className="size-3 text-brand-400" />
                        <span>Salvar</span>
                      </Button>
                    </div>
                  </div>
                </div>
              ) : detail?.caption ? (
                <div className="space-y-2">
                  <p className="rounded-lg border border-white/5 bg-black/50 p-2.5 text-xs text-zinc-200 font-mono select-text whitespace-pre-wrap leading-relaxed max-h-36 overflow-y-auto [scrollbar-width:thin]">
                    &ldquo;{detail.caption.text}&rdquo;
                  </p>
                  <div className="flex items-center justify-end gap-1.5">
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      onClick={() => copyText(detail.caption!.text)}
                      title="Copiar texto da legenda"
                    >
                      <IconCopy className="size-3" />
                      <span>Copiar</span>
                    </Button>
                    <Button
                      type="button"
                      variant="secondary"
                      size="sm"
                      onClick={() => {
                        setCaptionInput(detail.caption!.text);
                        setIsEditingCaption(true);
                      }}
                      title="Editar texto da legenda"
                    >
                      <span>Editar</span>
                    </Button>
                  </div>
                </div>
              ) : (
                <div className="space-y-2">
                  <p className="font-mono text-xs text-zinc-500 italic">
                    Nenhuma legenda vinculada a esta imagem.
                  </p>
                  <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    onClick={() => {
                      setCaptionInput("");
                      setIsEditingCaption(true);
                    }}
                    className="w-full justify-center"
                  >
                    <IconSparkles className="size-3 text-brand-400" />
                    <span>+ Adicionar Legenda</span>
                  </Button>
                </div>
              )}
            </div>

            {/* Classes presentes (para datasets YOLO) */}
            {dataset?.category === "yolo" && (
              <div>
                <p className="font-mono text-2xs text-zinc-400 uppercase tracking-caps font-semibold mb-2">
                  Classes nesta amostra:
                </p>
                {loading ? (
                  <p className="font-mono text-xs text-zinc-500">Carregando classes…</p>
                ) : boxes.length === 0 ? (
                  <p className="font-mono text-xs text-zinc-500 italic">Nenhuma anotação nesta imagem.</p>
                ) : (
                  <div className="flex flex-wrap gap-1.5">
                    {Array.from(new Set(boxes.map((b) => b.classId))).map((clsId) => {
                      const cls = classesMap.get(clsId);
                      const color = cls?.color ?? "#8350f2";
                      const count = boxes.filter((b) => b.classId === clsId).length;
                      return (
                        <span
                          key={clsId}
                          style={{ borderColor: `${color}40`, backgroundColor: `${color}15`, color }}
                          className="inline-flex items-center space-x-1.5 rounded-md border px-2 py-0.5 font-mono text-2xs font-medium"
                        >
                          <span className="size-1.5 rounded-full" style={{ backgroundColor: color }} />
                          <span>{cls?.name ?? "tag"}</span>
                          <span className="opacity-70 text-3xs">({count})</span>
                        </span>
                      );
                    })}
                  </div>
                )}
              </div>
            )}

            {/* Ação Copiar Nome/Hash */}
            <Button
              type="button"
              variant="secondary"
              size="sm"
              onClick={() => copyText(currentItem.filename)}
              className="w-full justify-center"
            >
              <IconCopy className="size-3.5" />
              <span>{copied ? "Copiado!" : "Copiar nome canônico"}</span>
            </Button>
          </div>

          {/* Ações Inferiores */}
          <div className="mt-6 space-y-2 pt-4 border-t border-zinc-800/80">
            {dataset?.category === "yolo" && (
              <Button
                type="button"
                variant="primary"
                size="md"
                onClick={() => {
                  if (onEditImage) {
                    onEditImage(currentItem);
                  } else if (dataset) {
                    router.push(`/datasets/${dataset.id}/annotate/${currentItem.id}`);
                  }
                }}
                className="w-full justify-center"
              >
                <IconTarget className="size-4" />
                <span>Abrir no Editor BBox</span>
              </Button>
            )}
            {onDelete && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  onClose();
                  onDelete(currentItem);
                }}
                className="w-full justify-center text-rose-400 hover:text-rose-300 hover:bg-rose-500/10"
              >
                <IconTrash className="size-3.5" />
                <span>Mover para Lixeira</span>
              </Button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export default ImageQuickLookModal;
