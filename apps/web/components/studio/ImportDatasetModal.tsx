"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconDownload, IconX } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { importDataset, importErrorMessage } from "@/lib/backup";
import type { Dataset } from "@/types/studio";
import { showToast } from "./Toast";

interface Props {
  onClose: () => void;
}

type Phase = "form" | "confirm";

export default function ImportDatasetModal({ onClose }: Props) {
  const router = useRouter();
  const fileRef = useRef<HTMLInputElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);
  const [file, setFile] = useState<File | null>(null);
  const [title, setTitle] = useState("");
  const [phase, setPhase] = useState<Phase>("form");
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  useEffect(() => {
    const t = setTimeout(() => titleRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, []);

  useEffect(() => {
    if (busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  async function runImport(replace: boolean): Promise<Dataset | null> {
    if (!file) return null;
    setBusy(true);
    setTopError(null);
    try {
      const trimmed = title.trim();
      const created = await importDataset(file, {
        ...(trimmed ? { title: trimmed.slice(0, 96) } : {}),
        ...(replace ? { replace: true } : {}),
      });
      showToast(
        `Dataset importado — ${created.imagesCount.toLocaleString()} ${created.imagesCount === 1 ? "imagem" : "imagens"}, ${created.classes.length.toLocaleString()} ${created.classes.length === 1 ? "classe" : "classes"}.`,
        "success",
        {
          label: "Abrir dataset importado",
          onClick: () => router.push(`/datasets/${created.id}`),
        },
      );
      onClose();
      return created;
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return null;
        }
        if (err.status === 413) {
          showToast("Backup maior que o limite de 200 MiB.", "error");
          return null;
        }
        if (err.status === 409 && err.code === "slug_conflict" && !replace) {
          // Detecção → diálogo de irreversibilidade (D5); arquivo preservado.
          setPhase("confirm");
          return null;
        }
        setTopError(importErrorMessage(err.code));
        return null;
      }
      setTopError("Falha ao importar backup.");
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!file) {
      setTopError("Escolha um arquivo .zip de backup.");
      return;
    }
    await runImport(false);
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
        aria-labelledby="import-dataset-title"
        className="glass-modal relative w-full max-w-lg rounded-2xl p-6 text-zinc-100 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-white/10 pb-4">
          <div className="flex items-center space-x-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
              <IconDownload className="h-4 w-4" />
            </div>
            <h3 id="import-dataset-title" className="text-sm font-bold text-white">
              {phase === "confirm" ? "Substituir dataset" : "Importar backup"}
            </h3>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar modal"
            className="rounded-lg border border-transparent bg-transparent p-1 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        {phase === "confirm" ? (
          <div className="mt-4 space-y-4 text-xs">
            <p role="alert" className="text-xs leading-relaxed text-zinc-300">
              Já existe um dataset com este nome. Substituir apaga o dataset
              atual e recria a partir do backup — a ação não tem reversão.
            </p>
            <div className="flex justify-end space-x-2 pt-2">
              <button
                type="button"
                onClick={() => setPhase("form")}
                disabled={busy}
                className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-4 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
              >
                Cancelar
              </button>
              <button
                type="button"
                onClick={() => runImport(true)}
                disabled={busy}
                className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
              >
                {busy ? "Substituindo…" : "Substituir"}
              </button>
            </div>
          </div>
        ) : (
          <form onSubmit={handleSubmit} className="mt-4 space-y-4 text-xs">
            {topError && (
              <p role="alert" className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300">
                {topError}
              </p>
            )}
            <div>
              <label htmlFor="import-dataset-file" className="mb-1 block font-medium text-zinc-300">
                Arquivo de backup (.zip)
              </label>
              <input
                id="import-dataset-file"
                ref={fileRef}
                type="file"
                accept=".zip"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
                className="w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-zinc-200 file:mr-3 file:rounded-lg file:border-0 file:bg-zinc-800 file:px-3 file:py-1.5 file:text-xs file:font-medium file:text-zinc-200 focus:border-brand-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
              />
              {file && (
                <p className="mt-1 font-mono text-[11px] text-zinc-500">
                  {file.name}
                </p>
              )}
            </div>
            <div>
              <label htmlFor="import-dataset-name" className="mb-1 block font-medium text-zinc-300">
                Nome do dataset (opcional)
              </label>
              <input
                id="import-dataset-name"
                ref={titleRef}
                type="text"
                maxLength={96}
                placeholder="Importar com outro nome"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                className="w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
              />
              <p className="mt-1 text-[11px] text-zinc-500">
                Vazio usa o nome do backup — máx 96 caracteres.
              </p>
            </div>
            <div className="flex justify-end space-x-2 pt-2">
              <button
                type="button"
                onClick={onClose}
                disabled={busy}
                className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-4 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
              >
                Cancelar
              </button>
              <button
                type="submit"
                disabled={busy || !file}
                className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
              >
                {busy ? "Importando…" : "Importar"}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
