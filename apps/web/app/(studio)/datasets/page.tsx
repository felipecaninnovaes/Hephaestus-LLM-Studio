"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import DatasetCard from "@/components/studio/DatasetCard";
import DatasetTable from "@/components/studio/DatasetTable";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import CreateDatasetModal from "@/components/studio/CreateDatasetModal";
import DatasetMenu from "@/components/studio/DatasetMenu";
import { showToast } from "@/components/studio/Toast";
import {
  IconDatabase,
  IconDownload,
  IconGrid,
  IconList,
  IconPlus,
  IconSearch,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { deleteDataset, listDatasets } from "@/lib/datasets";
import type { Dataset, DatasetCategory } from "@/types/studio";

type ViewMode = "grid" | "list";
type Pill = "all" | DatasetCategory;

const PILLS: { id: Pill; label: string }[] = [
  { id: "all", label: "Todos" },
  { id: "difusao", label: "Difusão" },
  { id: "openclip", label: "OpenCLIP" },
  { id: "yolo", label: "YOLO" },
];

function errorMessage(code: string): string {
  switch (code) {
    case "not_found":
      return "Datasets não encontrados.";
    case "validation":
      return "Requisição inválida ao carregar datasets.";
    default:
      return "Falha ao carregar datasets.";
  }
}

export default function DatasetsPage() {
  const router = useRouter();
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("grid");
  const [query, setQuery] = useState("");
  const [pill, setPill] = useState<Pill>("all");
  const [createOpen, setCreateOpen] = useState(false);
  const [deleting, setDeleting] = useState<Dataset | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [menu, setMenu] = useState<{ dataset: Dataset; x: number; y: number } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await listDatasets();
      setDatasets(data);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      setError(
        err instanceof ApiError ? errorMessage(err.code) : errorMessage(""),
      );
    } finally {
      setLoading(false);
    }
  }, [router]);

  useEffect(() => {
    load();
  }, [load]);

  const counts = useMemo(() => {
    const c: Record<Pill, number> = {
      all: datasets.length,
      difusao: 0,
      openclip: 0,
      yolo: 0,
    };
    for (const d of datasets) c[d.category] += 1;
    return c;
  }, [datasets]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return datasets.filter((d) => {
      if (pill !== "all" && d.category !== pill) return false;
      if (!q) return true;
      return (
        d.title.toLowerCase().includes(q) ||
        d.slug.toLowerCase().includes(q) ||
        d.classes.some((c) => c.name.toLowerCase().includes(q))
      );
    });
  }, [datasets, pill, query]);

  function clearFilters() {
    setQuery("");
    setPill("all");
  }

  function handleContextMenu(dataset: Dataset, x: number, y: number) {
    setMenu({ dataset, x, y });
  }

  async function confirmDelete() {
    if (!deleting) return;
    const target = deleting;
    setDeleteBusy(true);
    try {
      await deleteDataset(target.id);
      setDatasets((prev) => prev.filter((d) => d.id !== target.id));
      showToast("Dataset excluído.", "success");
      setDeleting(null);
    } catch (err) {
      if (err instanceof ApiError && (err.code === "unauthorized" || err.status === 401)) {
        router.replace("/login");
        return;
      }
      if (err instanceof ApiError && err.code === "not_found") {
        setDatasets((prev) => prev.filter((d) => d.id !== target.id));
        showToast("Dataset já não existe.", "info");
        setDeleting(null);
        return;
      }
      showToast("Falha ao excluir dataset.", "error");
    } finally {
      setDeleteBusy(false);
    }
  }

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
      <div className="flex items-center justify-between gap-3">
        <div className="flex items-baseline gap-3">
          <h1 className="tracking-display text-base font-semibold text-zinc-100 lg:text-lg">
            Gerenciador de Datasets
          </h1>
          <span className="font-mono text-xs text-zinc-500">
            {datasets.length} datasets
          </span>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            disabled
            title="Import chega na fatia 3e"
            className="flex items-center gap-1.5 rounded-xl border border-zinc-700/80 bg-zinc-900/60 px-3 py-2 text-xs font-medium text-zinc-400 opacity-60"
          >
            <IconDownload className="h-4 w-4" />
            Importar
          </button>
          <button
            type="button"
            onClick={() => setCreateOpen(true)}
            className="flex items-center gap-1.5 rounded-xl bg-emerald-500 px-3 py-2 text-xs font-semibold text-zinc-950 shadow-lg shadow-emerald-500/20"
          >
            <IconPlus className="h-4 w-4" />
            Novo Dataset
          </button>
          <div className="flex items-center gap-1 rounded-full border border-zinc-800 bg-zinc-900/60 p-1">
          <button
            type="button"
            aria-pressed={viewMode === "grid"}
            aria-label="Ver em grade"
            onClick={() => setViewMode("grid")}
            className={`rounded-full p-1.5 transition-colors ${viewMode === "grid" ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"}`}
          >
            <IconGrid className="h-4 w-4" />
          </button>
          <button
            type="button"
            aria-pressed={viewMode === "list"}
            aria-label="Ver em lista"
            onClick={() => setViewMode("list")}
            className={`rounded-full p-1.5 transition-colors ${viewMode === "list" ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300"}`}
          >
            <IconList className="h-4 w-4" />
          </button>
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <label className="relative block w-full sm:max-w-xs">
          <IconSearch className="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-zinc-500" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Buscar por nome, slug ou classe…"
            className="w-full rounded-xl border border-zinc-800 bg-zinc-900/60 py-2 pr-3 pl-9 text-sm text-zinc-100 placeholder:text-zinc-500"
          />
        </label>
        <div className="flex flex-wrap gap-1.5">
          {PILLS.map((p) => (
            <button
              key={p.id}
              type="button"
              onClick={() => setPill(p.id)}
              aria-pressed={pill === p.id}
              className={`rounded-full border px-3 py-1.5 text-xs font-medium transition-colors ${
                pill === p.id
                  ? "border-emerald-400/50 bg-emerald-400/10 text-emerald-300"
                  : "border-zinc-800 bg-zinc-900/60 text-zinc-400 hover:text-zinc-200"
              }`}
            >
              {p.label}{" "}
              <span className="font-mono text-[11px] opacity-70">
                {counts[p.id]}
              </span>
            </button>
          ))}
        </div>
      </div>

      {loading ? (
        <p className="py-10 text-center font-mono text-xs text-zinc-500">
          Carregando datasets…
        </p>
      ) : error ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error}</p>
          <button
            type="button"
            onClick={load}
            className="rounded-lg border border-zinc-700/80 bg-zinc-900/60 px-4 py-2 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-800"
          >
            Tentar novamente
          </button>
        </div>
      ) : datasets.length === 0 ? (
        <div className="glass-card flex flex-col items-center gap-2 rounded-2xl p-12 text-center">
          <IconDatabase className="h-10 w-10 text-zinc-700" />
          <p className="text-sm font-medium text-zinc-200">
            Nenhum dataset ainda
          </p>
          <p className="text-xs text-zinc-500">
            Crie seu primeiro dataset para começar a anotar.
          </p>
        </div>
      ) : filtered.length === 0 ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-12 text-center">
          <p className="text-sm font-medium text-zinc-200">Nada encontrado</p>
          <button
            type="button"
            onClick={clearFilters}
            className="rounded-lg border border-zinc-700/80 bg-zinc-900/60 px-4 py-2 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-800"
          >
            Limpar Filtros
          </button>
        </div>
      ) : viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {filtered.map((d) => (
            <DatasetCard key={d.id} dataset={d} onContextMenu={handleContextMenu} />
          ))}
        </div>
      ) : (
        <div className="glass-card rounded-2xl p-2 sm:p-3">
          <DatasetTable datasets={filtered} onContextMenu={handleContextMenu} />
        </div>
      )}

      <CreateDatasetModal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        onCreated={(created) =>
          setDatasets((prev) => [created, ...prev.filter((d) => d.id !== created.id)])
        }
      />
      <ConfirmDialog
        open={deleting !== null}
        title="Excluir dataset"
        body={
          deleting ? (
            <p>
              Excluir <strong className="text-zinc-100">{deleting.title}</strong>{" "}
              (<span className="font-mono">{deleting.slug}</span>)? As imagens do
              dataset serão removidas do storage. Essa ação não pode ser desfeita.
            </p>
          ) : null
        }
        confirmLabel="Excluir"
        danger
        busy={deleteBusy}
        onConfirm={confirmDelete}
        onClose={() => {
          if (!deleteBusy) setDeleting(null);
        }}
      />
      {menu && (
        <DatasetMenu
          dataset={menu.dataset}
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          onDelete={(d) => setDeleting(d)}
        />
      )}
    </div>
  );
}
