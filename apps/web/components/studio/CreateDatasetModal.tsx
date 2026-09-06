"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconPlus, IconX } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { CLASS_RE, MAX_CLASSES } from "@/lib/classes";
import { createDataset } from "@/lib/datasets";
import { TYPE_LABELS, type Dataset, type DatasetType } from "@/types/studio";
import { showToast } from "./Toast";
const TYPES = Object.keys(TYPE_LABELS) as DatasetType[];

export function slugPreview(title: string): string {
  return title
    .toLowerCase()
    .trim()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/\s+/g, "-")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (dataset: Dataset) => void;
}

export default function CreateDatasetModal({ open, onClose, onCreated }: Props) {
  const router = useRouter();
  const nameRef = useRef<HTMLInputElement>(null);
  const [title, setTitle] = useState("");
  const [type, setType] = useState<DatasetType>("yolo_bbox");
  const [classesRaw, setClassesRaw] = useState("");
  const [nameError, setNameError] = useState<string | null>(null);
  const [classesError, setClassesError] = useState<string | null>(null);
  const [topError, setTopError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setTitle("");
    setType("yolo_bbox");
    setClassesRaw("");
    setNameError(null);
    setClassesError(null);
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => nameRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open ]);

  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  const slug = slugPreview(title);
  const simpleSlug = title.toLowerCase().trim().replace(/\s+/g, "-");
  const showAdjustHint = slug.length > 0 && slug !== simpleSlug;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setNameError(null);
    setClassesError(null);
    setTopError(null);
    const trimmed = title.trim();
    if (!trimmed || trimmed.length > 96) {
      setNameError("Dê um nome ao dataset (máx 96 caracteres).");
      return;
    }
    const seen = new Set<string>();
    const classes: string[] = [];
    for (const part of classesRaw.split(",")) {
      const name = part.trim();
      if (!name || seen.has(name)) continue;
      seen.add(name);
      classes.push(name);
    }
    const bad = classes.find((c) => !CLASS_RE.test(c));
    if (bad) {
      setClassesError("Classe inválida: use letras, números e _ (máx 64).");
      return;
    }
    if (classes.length > MAX_CLASSES) {
      setClassesError("Máximo de 200 classes por dataset.");
      return;
    }
    setBusy(true);
    try {
      const created = await createDataset(
        classes.length > 0 ? { title: trimmed, type, classes } : { title: trimmed, type },
      );
      showToast("Dataset criado.", "success");
      onClose();
      onCreated(created);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "slug_conflict") {
          setNameError("Já existe um dataset com esse nome.");
          return;
        }
        if (err.code === "invalid_request") {
          setTopError("Verifique os campos.");
          return;
        }
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
      }
      setTopError("Falha inesperada.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-dataset-title"
        className="glass-modal relative w-full max-w-lg rounded-2xl p-6 text-zinc-100 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-white/10 pb-4">
          <div className="flex items-center space-x-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-emerald-500/30 bg-emerald-500/15 text-emerald-400">
              <IconPlus className="h-4 w-4" />
            </div>
            <h3 id="create-dataset-title" className="text-sm font-bold text-white">
              Novo Dataset
            </h3>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar modal"
            className="rounded-lg p-1 text-zinc-400 transition-colors hover:text-white"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="mt-4 space-y-4 text-xs">
          {topError && (
            <p role="alert" className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
              {topError}
            </p>
          )}
          <div>
            <label htmlFor="create-dataset-name" className="mb-1 block font-medium text-zinc-300">
              Nome
            </label>
            <input
              id="create-dataset-name"
              ref={nameRef}
              type="text"
              required
              maxLength={96}
              placeholder="Ex.: Inspeção de PCB v2"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className="w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-zinc-200 focus:border-emerald-500 focus:outline-none"
            />
            {slug && (
              <p className="mt-1 font-mono text-[11px] text-zinc-500">
                slug: {slug}
                {showAdjustHint && " · o servidor pode ajustar"}
              </p>
            )}
            {nameError && (
              <p role="alert" className="mt-1 text-[11px] text-rose-300">
                {nameError}
              </p>
            )}
          </div>

          <div>
            <label htmlFor="create-dataset-type" className="mb-1 block font-medium text-zinc-300">
              Tipo / Tarefa
            </label>
            <select
              id="create-dataset-type"
              value={type}
              onChange={(e) => setType(e.target.value as DatasetType)}
              className="w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-zinc-200"
            >
              {TYPES.map((t) => (
                <option key={t} value={t}>
                  {TYPE_LABELS[t]}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label htmlFor="create-dataset-classes" className="mb-1 block font-medium text-zinc-300">
              Classes (opcional)
            </label>
            <input
              id="create-dataset-classes"
              type="text"
              placeholder="defeito_a, defeito_b, anomalia"
              value={classesRaw}
              onChange={(e) => setClassesRaw(e.target.value)}
              className="w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-zinc-200 focus:border-emerald-500 focus:outline-none"
            />
            <p className="mt-1 text-[11px] text-zinc-500">
              Separadas por vírgula — ordem vira índice YOLO
            </p>
            {classesError && (
              <p role="alert" className="mt-1 text-[11px] text-rose-300">
                {classesError}
              </p>
            )}
          </div>

          <div className="flex justify-end space-x-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              disabled={busy}
              className="rounded-xl bg-zinc-900 px-4 py-2 font-medium text-zinc-300 hover:bg-zinc-800"
            >
              Cancelar
            </button>
            <button
              type="submit"
              disabled={busy}
              className="rounded-xl bg-emerald-500 px-5 py-2 font-semibold text-zinc-950 shadow-lg shadow-emerald-500/20 active:scale-[0.98] disabled:opacity-60"
            >
              {busy ? "Criando…" : "Criar Dataset"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
