"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconPlus, IconX } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { ApiError } from "@/lib/api";
import { CLASS_RE, MAX_CLASSES, putClasses } from "@/lib/classes";
import { getDataset } from "@/lib/datasets";
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
  const deletedIdsRef = useRef<Set<string>>(new Set());
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

  // Sincroniza classes frescas do servidor em background ao abrir
  useEffect(() => {
    let active = true;
    getDataset(datasetId)
      .then((ds) => {
        if (!active) return;
        const freshRows = toRows(ds.classes);
        setRows((prev) => {
          const userAddedRows = prev.filter((r) => !r.id);
          // Se o usuário não adicionou nem excluiu nada ainda, espelha o servidor
          if (userAddedRows.length === 0 && deletedIdsRef.current.size === 0) {
            initialRef.current = JSON.stringify(
              freshRows.map((r) => ({ id: r.id ?? null, name: r.name })),
            );
            return freshRows;
          }
          // Caso contrário, mescla classes do servidor que não estejam em prev nem em deletedIdsRef
          const existingIds = new Set(prev.filter((r) => r.id).map((r) => r.id as string));
          const missingFromServer = freshRows.filter(
            (fr) => fr.id && !existingIds.has(fr.id) && !deletedIdsRef.current.has(fr.id),
          );
          if (missingFromServer.length > 0) {
            return [...prev, ...missingFromServer];
          }
          return prev;
        });
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [datasetId]);

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
    const row = rows.find((r) => r.key === key);
    if (row?.id) {
      deletedIdsRef.current.add(row.id);
    }
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

    setBusy(true);

    // Consulta classes frescas para não omitir acidentalmente classes criadas em background (ex: autotracker)
    let extraServerClasses: { id: string; name: string }[] = [];
    try {
      const fresh = await getDataset(datasetId);
      const currentIds = new Set(rows.filter((r) => r.id).map((r) => r.id as string));
      extraServerClasses = fresh.classes
        .filter((c) => !currentIds.has(c.id) && !deletedIdsRef.current.has(c.id))
        .map((c) => ({ id: c.id, name: c.name }));
    } catch {
      // Se falhar o reload pré-flight, prossegue com os dados locais
    }

    const payload: PutClassInput[] = [
      ...rows.map((r) =>
        r.id ? { id: r.id, name: r.name.trim() } : { name: r.name.trim() },
      ),
      ...extraServerClasses,
    ];

    try {
      const res = await putClasses(datasetId, payload);
      showToast("Classes salvas.", "success");
      onSaved(res.classes);
      onClose();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "classes_in_use" || err.status === 409) {
          setFormError(
            "Conflito: há anotações usando uma das classes removidas. Sincronizando com o servidor...",
          );
          try {
            const fresh = await getDataset(datasetId);
            setRows(toRows(fresh.classes));
            deletedIdsRef.current.clear();
            initialRef.current = JSON.stringify(
              toRows(fresh.classes).map((r) => ({ id: r.id ?? null, name: r.name })),
            );
          } catch {}
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
    <Modal
      open={true}
      onClose={onClose}
      title="Gerenciar classes"
      maxWidth="md"
      busy={busy}
    >
      <form onSubmit={handleSubmit} className="space-y-3 text-xs">
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
                    background: row.color ?? "#585164",
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
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeRow(row.key)}
                  aria-label={`Remover classe ${i + 1}`}
                >
                  <IconX className="size-4" />
                </Button>
              </div>
            ))}
          </div>

          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={addRow}
            disabled={busy || rows.length >= MAX_CLASSES}
            leftIcon={<IconPlus className="size-4" />}
          >
            Adicionar classe
          </Button>

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
              disabled={busy || unchanged}
              loading={busy}
              title={unchanged ? "Nenhuma alteração para salvar." : undefined}
            >
              Salvar classes
            </Button>
          </div>
        </form>
    </Modal>
  );
}
