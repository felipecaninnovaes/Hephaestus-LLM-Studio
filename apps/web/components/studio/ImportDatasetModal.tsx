"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconDownload } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
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
    <Modal
      open={true}
      onClose={onClose}
      title={phase === "confirm" ? "Substituir dataset" : "Importar backup"}
      icon={<IconDownload className="h-4 w-4" />}
      maxWidth="lg"
      busy={busy}
    >

        {phase === "confirm" ? (
          <div className="mt-4 space-y-4 text-xs">
            <p role="alert" className="text-xs leading-relaxed text-zinc-300">
              Já existe um dataset com este nome. Substituir apaga o dataset
              atual e recria a partir do backup — a ação não tem reversão.
            </p>
            <div className="flex justify-end space-x-2 pt-2">
              <Button
                type="button"
                variant="ghost"
                size="md"
                onClick={() => setPhase("form")}
                disabled={busy}
              >
                Cancelar
              </Button>
              <Button
                type="button"
                variant="destructive"
                size="lg"
                onClick={() => runImport(true)}
                loading={busy}
              >
                Substituir
              </Button>
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
                <p className="mt-1 font-mono text-2xs text-zinc-500">
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
              <p className="mt-1 text-2xs text-zinc-500">
                Vazio usa o nome do backup — máx 96 caracteres.
              </p>
            </div>
            <div className="flex justify-end space-x-2 pt-2">
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
                type="submit"
                variant="primary"
                size="lg"
                disabled={busy || !file}
                loading={busy}
              >
                Importar
              </Button>
            </div>
          </form>
        )}
    </Modal>
  );
}
