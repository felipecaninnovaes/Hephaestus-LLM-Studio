"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconPlus, IconX } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { CLASS_RE, MAX_CLASSES, putClasses } from "@/lib/classes";
import type { PutClassInput, StudioClass } from "@/types/studio";
import { showToast } from "./Toast";

interface Props {
  datasetId: string;
  datasetClasses: StudioClass[];
  onClose: () => void;
  onSaved: (classes: StudioClass[]) => void;
}

interface Row {
  id?: string;
  name: string;
  key: string;
  color: string | null;
}

let rowSeq = 0;
function newKey(): string {
  rowSeq += 1;
  return `nova-${rowSeq}`;
}

function toRows(classes: StudioClass[]): Row[] {
  return [...classes]
    .sort((a, b) => a.idx - b.idx)
    .map((c) => ({ id: c.id, name: c.name, key: c.id, color: c.color }));
}

export default function ClassesModal({
  datasetId,
  datasetClasses,
  onClose,
  onSaved,
}: Props) {
  const router = useRouter();
  const firstRef = useRef<HTMLInputElement>(null);
  const [rows, setRows] = useState<Row[]>(() => toRows(datasetClasses));
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const initialRef = useRef<string>("");
  if (!initialRef.current) {
    initialRef.current = JSON.stringify(
      toRows(datasetClasses).map((r) => ({ id: r.id ?? null, name: r.name })),
    );
  }

  useEffect(() => {
    const t = setTimeout(() => firstRef.current?.focus(), 30);
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

  const currentSig = useMemo(
    () =>
      JSON.stringify(
        rows.map((r) => ({ id: r.id ?? null, name: r.name.trim() })),
      ),
    [rows],
  );
  const unchanged = currentSig === initialRef.current;

  function setName(key: string, name: string) {
    setRows((prev) => prev.map((r) => (r.key === key ? { ...r, name } : r)));
  }

  function removeRow(key: string) {
    setRows((prev) => prev.filter((r) => r.key !== key));
  }

  function addRow() {
    if (rows.length >= MAX_CLASSES) {
      setFormError("Máximo de 200 classes por dataset.");
      return;
    }
    setFormError(null);
    setRows((prev) => [...prev, { name: "", key: newKey(), color: null }]);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setFormError(null);
    const names = rows.map((r) => r.name.trim());
    const emptyIdx = names.findIndex((n) => n.length === 0);
    if (emptyIdx >= 0) {
      setFormError(
        `Linha ${emptyIdx + 1}: preencha o nome ou remova a linha.`,
      );
      return;
    }
    const bad = names.find((n) => !CLASS_RE.test(n));
    if (bad) {
      setFormError("Classe inválida: use letras, números e _ (máx 64).");
      return;
    }
    const seen = new Set<string>();
    if (names.some((n) => (seen.has(n) ? true : (seen.add(n), false)))) {
      setFormError("Há nomes de classe duplicados.");
      return;
    }
    if (names.length > MAX_CLASSES) {
      setFormError("Máximo de 200 classes por dataset.");
      return;
    }
    const payload: PutClassInput[] = rows.map((r) =>
      r.id ? { id: r.id, name: r.name.trim() } : { name: r.name.trim() },
    );
    setBusy(true);
    try {
      const res = await putClasses(datasetId, payload);
      showToast("Classes salvas.", "success");
      onSaved(res.classes);
      onClose();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "classes_in_use" || err.status === 409) {
          showToast(
            "Há anotações usando uma das classes removidas.",
            "error",
          );
          return;
        }
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
        showToast(err.message || "Falha ao salvar classes.", "error");
        return;
      }
      showToast("Falha ao salvar classes.", "error");
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
        aria-labelledby="classes-modal-title"
        className="glass-modal relative w-full max-w-md rounded-2xl p-6 text-zinc-100 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-white/10 pb-4">
          <h3 id="classes-modal-title" className="text-sm font-bold text-white">
            Gerenciar classes
          </h3>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar modal"
            className="rounded-lg border border-transparent bg-transparent p-1 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="mt-4 space-y-3 text-xs">
          {formError && (
            <p
              role="alert"
              className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
            >
              {formError}
            </p>
          )}

          <div className="max-h-72 space-y-2 overflow-y-auto pr-1">
            {rows.length === 0 && (
              <p className="rounded-xl border border-dashed border-zinc-700 px-3 py-4 text-center text-zinc-400">
                Sem classes — adicione a primeira abaixo ou salve vazio para
                remover todas.
              </p>
            )}
            {rows.map((row, i) => (
              <div key={row.key} className="flex items-center gap-2">
                <span
                  className="h-2.5 w-2.5 shrink-0 rounded-full"
                  style={{
                    background: row.color ?? "#52525b",
                  }}
                  aria-hidden="true"
                />
                <input
                  ref={i === 0 ? firstRef : undefined}
                  type="text"
                  value={row.name}
                  onChange={(e) => setName(row.key, e.target.value)}
                  placeholder="nome_da_classe"
                  maxLength={64}
                  aria-label={`Classe ${i + 1}`}
                  className="h-9 w-full rounded-xl border border-zinc-700/80 bg-zinc-900 px-3 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
                />
                <button
                  type="button"
                  onClick={() => removeRow(row.key)}
                  aria-label={`Remover classe ${i + 1}`}
                  className="inline-flex size-9 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                >
                  <IconX className="h-4 w-4" />
                </button>
              </div>
            ))}
          </div>

          <button
            type="button"
            onClick={addRow}
            disabled={busy || rows.length >= MAX_CLASSES}
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-3 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconPlus className="h-4 w-4" />
            <span>Adicionar classe</span>
          </button>

          <div className="flex justify-end space-x-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              disabled={busy}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-4 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              Cancelar
            </button>
            <button
              type="submit"
              disabled={busy || unchanged}
              title={unchanged ? "Nenhuma alteração para salvar." : undefined}
              className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              {busy ? "Salvando…" : "Salvar classes"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
