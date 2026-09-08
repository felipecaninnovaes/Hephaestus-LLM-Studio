"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import DatasetCard from "@/components/studio/DatasetCard";
import DatasetTable from "@/components/studio/DatasetTable";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import CreateDatasetModal from "@/components/studio/CreateDatasetModal";
import DatasetMenu from "@/components/studio/DatasetMenu";
import { showToast } from "@/components/studio/Toast";
import {
  IconDatabase,
  IconGrid,
  IconList,
  IconPlay,
  IconPlus,
  IconSearch,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { deleteDataset, listDatasets } from "@/lib/datasets";
import TrainYoloModal from "@/components/studio/TrainYoloModal";
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
    case "invalid_request":
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
  const [trainDataset, setTrainDataset] = useState<Dataset | null>(null);
  const pillsRef = useRef<HTMLDivElement>(null);

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

  useEffect(() => {
    pillsRef.current
      ?.querySelector('[data-active="true"]')
      ?.scrollIntoView({ inline: "nearest", block: "nearest" });
  }, [pill]);

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
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="min-w-0">
          <div className="flex items-baseline gap-3">
            <h1 className="font-display tracking-display truncate text-xl font-semibold text-zinc-100 lg:text-2xl" title="Gerenciador de Datasets">
              Gerenciador de Datasets
            </h1>
            <span className="shrink-0 font-mono text-xs text-zinc-500">
              {datasets.length} datasets
            </span>
          </div>
          <p className="mt-0.5 text-xs text-zinc-400">
            Repositório unificado por categoria: Difusão, OpenCLIP e YOLO. Clique em um dataset para abrir a galeria.
          </p>
        </div>
        <div className="flex min-h-[44px] items-center gap-2">
          <div className="inline-flex rounded-full border border-white/10 bg-black/40 p-1" role="group" aria-label="Modo de visualização">
            <button
              type="button"
              aria-pressed={viewMode === "grid"}
              aria-label="Ver em grade"
              title="Ver em grade"
              onClick={() => setViewMode("grid")}
              className={`inline-flex h-7 items-center justify-center rounded-full px-2.5 transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 ${viewMode === "grid" ? "bg-brand-500/[0.18] text-brand-300" : "text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"}`}
            >
              <IconGrid className="h-4 w-4" />
            </button>
            <button
              type="button"
              aria-pressed={viewMode === "list"}
              aria-label="Ver em lista"
              title="Ver em lista"
              onClick={() => setViewMode("list")}
              className={`inline-flex h-7 items-center justify-center rounded-full px-2.5 transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 ${viewMode === "list" ? "bg-brand-500/[0.18] text-brand-300" : "text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"}`}
            >
              <IconList className="h-4 w-4" />
            </button>
          </div>
          <button
            type="button"
            onClick={() => setCreateOpen(true)}
            className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconPlus className="h-4 w-4" />
            Novo Dataset
          </button>
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
            aria-label="Buscar por nome, slug ou classe"
            className="w-full rounded-xl border border-zinc-800 bg-black/40 py-2 pr-3 pl-9 text-sm text-zinc-100 placeholder:text-zinc-500 focus:border-brand-500 focus:outline-none"
          />
        </label>
        <div className="relative min-w-0 flex-1 sm:flex-none">
          <div ref={pillsRef} className="no-scrollbar flex gap-1.5 overflow-x-auto py-0.5 pr-8">
            {PILLS.map((p) => {
              const label = `${p.label} ${counts[p.id]}`;
              const active = pill === p.id;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setPill(p.id)}
                  aria-pressed={active}
                  data-active={active}
                  title={label}
                  className={`inline-flex h-9 shrink-0 items-center gap-1.5 truncate rounded-lg border px-4 text-sm font-medium whitespace-nowrap transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 ${
                    active
                      ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                      : "border-white/[0.08] bg-white/[0.03] text-zinc-400 hover:bg-white/[0.05] hover:text-zinc-200"
                  }`}
                >
                  <span className="truncate">{p.label}</span>{" "}
                  <span className="font-mono text-[11px] opacity-70">
                    {counts[p.id]}
                  </span>
                </button>
              );
            })}
          </div>
          <div className="pointer-events-none absolute top-0 right-0 h-full w-8 bg-gradient-to-l from-zinc-950 to-transparent" aria-hidden="true" />
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
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            Tentar novamente
          </button>
        </div>
      ) : datasets.length === 0 ? (
        <div className="glass-card flex flex-col items-center gap-2 rounded-2xl p-12 text-center">
          <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
            <IconDatabase className="h-10 w-10 text-zinc-700" />
          </span>
          <p className="text-sm font-medium text-zinc-200">
            Nenhum dataset ainda
          </p>
          <p className="text-xs text-zinc-500">
            Crie seu primeiro dataset para começar a anotar.
          </p>
        </div>
      ) : filtered.length === 0 ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-12 text-center">
          <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
            <IconDatabase className="h-6 w-6" />
          </span>
          <p className="text-sm font-medium text-zinc-200">Nada encontrado</p>
          <button
            type="button"
            onClick={clearFilters}
            className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-white/10 bg-white/[0.05] px-4 text-xs font-medium whitespace-nowrap text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] transition hover:border-white/20 hover:bg-white/[0.10] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            Limpar Filtros
          </button>
        </div>
      ) : viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {filtered.map((d) => (
            <DatasetCard key={d.id} dataset={d} onContextMenu={handleContextMenu} onTrain={(d) => setTrainDataset(d)} />
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
          onTrain={(d) => setTrainDataset(d)}
        />
      )}
      {trainDataset && (
        <TrainYoloModal
          open
          datasetId={trainDataset.id}
          datasetTitle={trainDataset.title}
          onClose={() => setTrainDataset(null)}
          onJobCreated={() => setTrainDataset(null)}
        />
      )}
    </div>
  );
}
