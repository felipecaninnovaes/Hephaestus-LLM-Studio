"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import DatasetCard from "@/components/studio/DatasetCard";
import DatasetTable from "@/components/studio/DatasetTable";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import CreateDatasetModal from "@/components/studio/CreateDatasetModal";
import DatasetMenu from "@/components/studio/DatasetMenu";
import { showToast } from "@/components/studio/Toast";
import { Button, SearchInput, SegmentedControl, SubmodulePills } from "@/components/ui";
import {
  IconDatabase,
  IconGrid,
  IconList,
  IconPlay,
  IconPlus,
  IconUpload,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { deleteDataset, listDatasets } from "@/lib/datasets";
import { inspectDataTransfer, type InspectionResult } from "@/lib/dataset-inspector";
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
  const [createMode, setCreateMode] = useState<"empty" | "import">("empty");
  const [droppedInspection, setDroppedInspection] = useState<InspectionResult | null>(null);
  const [isDraggingPage, setIsDraggingPage] = useState(false);
  const dragCounterRef = useRef(0);
  const [deleting, setDeleting] = useState<Dataset | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [menu, setMenu] = useState<{ dataset: Dataset; x: number; y: number } | null>(null);
  const [trainDataset, setTrainDataset] = useState<Dataset | null>(null);

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

  const handleDragEnter = (e: React.DragEvent) => {
    e.preventDefault();
    dragCounterRef.current += 1;
    if (e.dataTransfer.items && e.dataTransfer.items.length > 0) {
      setIsDraggingPage(true);
    }
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    dragCounterRef.current -= 1;
    if (dragCounterRef.current <= 0) {
      setIsDraggingPage(false);
      dragCounterRef.current = 0;
    }
  };

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingPage(false);
    dragCounterRef.current = 0;
    try {
      const res = await inspectDataTransfer(e.dataTransfer);
      if (res) {
        setDroppedInspection(res);
        setCreateMode("import");
        setCreateOpen(true);
      }
    } catch {
      // continua
    }
  };

  return (
    <div
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      className="relative mx-auto flex w-full max-w-7xl flex-col gap-4 px-4 sm:px-6 py-6"
    >
      {/* Overlay Óptico de Drag & Drop Global */}
      {isDraggingPage && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 p-6 backdrop-blur-md transition-all animate-in fade-in"
          onDragOver={(e) => e.preventDefault()}
          onDragLeave={handleDragLeave}
          onDrop={handleDrop}
        >
          <div className="pointer-events-none flex flex-col items-center gap-4 rounded-3xl border-2 border-dashed border-brand-500/80 bg-brand-500/10 backdrop-blur-sm p-12 text-center shadow-[0_0_60px_rgba(131,80,242,0.3)]">
            <div className="flex h-16 w-16 items-center justify-center rounded-2xl border border-brand-500/40 bg-brand-500/20 backdrop-blur-sm text-brand-300">
              <IconUpload className="h-8 w-8" />
            </div>
            <div>
              <p className="font-display text-lg font-bold text-white">
                Solte o arquivo ZIP ou pasta aqui
              </p>
              <p className="font-mono text-xs text-brand-200/80 mt-1">
                Autodeteção imediata de classes, anotações e contagem de imagens
              </p>
            </div>
          </div>
        </div>
      )}

      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="min-w-0">
          <div className="flex items-baseline gap-3">
            <h1 className="font-display tracking-display truncate text-xl font-semibold text-zinc-100 lg:text-2xl" title="Gerenciador de Datasets">
              Gerenciador de Datasets
            </h1>
            <span className="shrink-0 font-mono text-xs text-zinc-400">
              {datasets.length} datasets
            </span>
          </div>
          <p className="mt-0.5 text-xs text-zinc-400">
            Repositório unificado por categoria: Difusão, OpenCLIP e YOLO. Clique em um dataset para abrir a galeria.
          </p>
        </div>
        <div className="flex min-h-[44px] items-center gap-2">
          <SegmentedControl<ViewMode>
            ariaLabel="Modo de visualização"
            value={viewMode}
            onChange={setViewMode}
            options={[
              {
                id: "grid",
                icon: <IconGrid className="h-4 w-4" />,
                title: "Ver em grade",
                ariaLabel: "Ver em grade",
              },
              {
                id: "list",
                icon: <IconList className="h-4 w-4" />,
                title: "Ver em lista",
                ariaLabel: "Ver em lista",
              },
            ]}
          />
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => {
              setCreateMode("import");
              setDroppedInspection(null);
              setCreateOpen(true);
            }}
          >
            <IconUpload className="h-4 w-4 text-zinc-400" />
            Importar
          </Button>
          <Button
            type="button"
            variant="primary"
            size="md"
            onClick={() => {
              setCreateMode("empty");
              setDroppedInspection(null);
              setCreateOpen(true);
            }}
          >
            <IconPlus className="h-4 w-4" />
            Novo Dataset
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="w-full sm:max-w-xs">
          <SearchInput
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onClear={() => setQuery("")}
            placeholder="Buscar por nome, slug ou classe…"
            aria-label="Buscar por nome, slug ou classe"
          />
        </div>
        <SubmodulePills<Pill>
          value={pill}
          onChange={setPill}
          items={PILLS.map((p) => ({
            id: p.id,
            label: p.label,
            count: counts[p.id],
          }))}
        />
      </div>

      {loading ? (
        <p className="py-10 text-center font-mono text-xs text-zinc-400">
          Carregando datasets…
        </p>
      ) : error ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">{error}</p>
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={load}
          >
            Tentar novamente
          </Button>
        </div>
      ) : datasets.length === 0 ? (
        <div
          onDragOver={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
          onDrop={async (e) => {
            e.preventDefault();
            e.stopPropagation();
            try {
              const res = await inspectDataTransfer(e.dataTransfer);
              if (res) {
                setDroppedInspection(res);
                setCreateMode("import");
                setCreateOpen(true);
              }
            } catch {
              // continua
            }
          }}
          className="glass-card group flex flex-col items-center gap-4 rounded-2xl border-2 border-dashed border-zinc-800 hover:border-brand-500/50 p-12 text-center transition-colors"
        >
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl border border-zinc-800 bg-zinc-900/80 backdrop-blur-sm text-zinc-400 group-hover:text-brand-400 group-hover:border-brand-500/40 transition-colors">
            <IconUpload className="h-6 w-6" />
          </div>
          <div>
            <p className="font-display text-base font-semibold text-zinc-100">
              Nenhum dataset cadastrado ainda
            </p>
            <p className="mt-1 max-w-md text-xs text-zinc-400">
              Arraste um pacote ZIP de backup ou pasta de imagens diretamente para cá para autodeteção de classes e ingestão imediata, ou inicie com um container vazio.
            </p>
          </div>
          <div className="flex items-center gap-3 pt-2">
            <Button
              type="button"
              variant="secondary"
              size="md"
              onClick={() => {
                setCreateMode("empty");
                setDroppedInspection(null);
                setCreateOpen(true);
              }}
            >
              <IconPlus className="h-4 w-4" />
              Container Vazio
            </Button>
            <Button
              type="button"
              variant="primary"
              size="md"
              onClick={() => {
                setCreateMode("import");
                setDroppedInspection(null);
                setCreateOpen(true);
              }}
            >
              <IconUpload className="h-4 w-4" />
              Importar ZIP / Pasta
            </Button>
          </div>
        </div>
      ) : filtered.length === 0 ? (
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-12 text-center">
          <span className="mx-auto mb-1 flex h-12 w-12 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400">
            <IconDatabase className="h-6 w-6" />
          </span>
          <p className="text-sm font-medium text-zinc-200">Nada encontrado</p>
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={clearFilters}
          >
            Limpar Filtros
          </Button>
        </div>
      ) : viewMode === "grid" ? (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-3 2xl:grid-cols-4">
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
        onClose={() => {
          setCreateOpen(false);
          setDroppedInspection(null);
        }}
        initialMode={createMode}
        initialInspection={droppedInspection}
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
