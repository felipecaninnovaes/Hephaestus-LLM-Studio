"use client";

import { useRouter } from "next/navigation";
import AutoLabelModal from "@/components/studio/AutoLabelModal";
import AutoTrackerModal from "@/components/studio/AutoTrackerModal";
import BatchEditClassesModal from "@/components/studio/BatchEditClassesModal";
import ClassesModal from "@/components/studio/ClassesModal";
import CreateDatasetModal from "@/components/studio/CreateDatasetModal";
import ImageQuickLookModal from "@/components/studio/ImageQuickLookModal";
import TrainYoloModal from "@/components/studio/TrainYoloModal";
import UploadAuditModal from "@/components/studio/UploadAuditModal";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import type {
  Dataset,
  ImageItem,
  StudioClass,
  UploadResultItem,
} from "@/types/studio";

export interface DatasetModalsProps {
  dataset: Dataset;
  items: ImageItem[];
  setItems: React.Dispatch<React.SetStateAction<ImageItem[]>>;
  classesOpen: boolean;
  setClassesOpen: (open: boolean) => void;
  importOpen: boolean;
  setImportOpen: (open: boolean) => void;
  trainOpen: boolean;
  setTrainOpen: (open: boolean) => void;
  autoTrackerOpen: boolean;
  setAutoTrackerOpen: (open: boolean) => void;
  autoLabelOpen: boolean;
  setAutoLabelOpen: (open: boolean) => void;
  batchEditClassesOpen: boolean;
  setBatchEditClassesOpen: (open: boolean) => void;
  selectedIds: Set<string>;
  auditModalOpen: boolean;
  setAuditModalOpen: (open: boolean) => void;
  lastUploadResults: UploadResultItem[] | null;
  quickLookIndex: number | null;
  setQuickLookIndex: (idx: number | null) => void;
  deleting: ImageItem | null;
  setDeleting: (item: ImageItem | null) => void;
  deleteBusy: boolean;
  onConfirmSoftDelete: () => void;
  batchDeleteOpen: boolean;
  setBatchDeleteOpen: (open: boolean) => void;
  batchDeleteBusy: boolean;
  onConfirmBatchDelete: () => void;
  purgeOpen: boolean;
  setPurgeOpen: (open: boolean) => void;
  purgeBusy: boolean;
  onConfirmPurge: () => void;
  trashTotal: number;
  onClassesSaved: (classes: StudioClass[]) => void;
  onTileClick: (item: ImageItem) => void;
  onRefresh: () => void;
}

export function DatasetModals({
  dataset,
  items,
  setItems,
  classesOpen,
  setClassesOpen,
  importOpen,
  setImportOpen,
  trainOpen,
  setTrainOpen,
  autoTrackerOpen,
  setAutoTrackerOpen,
  autoLabelOpen,
  setAutoLabelOpen,
  batchEditClassesOpen,
  setBatchEditClassesOpen,
  selectedIds,
  auditModalOpen,
  setAuditModalOpen,
  lastUploadResults,
  quickLookIndex,
  setQuickLookIndex,
  deleting,
  setDeleting,
  deleteBusy,
  onConfirmSoftDelete,
  batchDeleteOpen,
  setBatchDeleteOpen,
  batchDeleteBusy,
  onConfirmBatchDelete,
  purgeOpen,
  setPurgeOpen,
  purgeBusy,
  onConfirmPurge,
  trashTotal,
  onClassesSaved,
  onTileClick,
  onRefresh,
}: DatasetModalsProps) {
  const router = useRouter();

  return (
    <>
      <UploadAuditModal
        open={auditModalOpen}
        results={lastUploadResults ?? []}
        onClose={() => setAuditModalOpen(false)}
      />

      <ImageQuickLookModal
        open={quickLookIndex !== null}
        dataset={dataset}
        items={items}
        currentIndex={quickLookIndex ?? 0}
        onClose={() => setQuickLookIndex(null)}
        onNavigate={(idx) => setQuickLookIndex(idx)}
        onDelete={(img) => setDeleting(img)}
        onEditImage={(img) => onTileClick(img)}
        onCaptionUpdated={(imgId, text) => {
          setItems((prev) =>
            prev.map((it) => (it.id === imgId ? { ...it, caption: text } : it)),
          );
        }}
      />

      {/* Confirmação de exclusão em lote */}
      <ConfirmDialog
        open={batchDeleteOpen}
        title="Mover imagens selecionadas para a lixeira"
        body={
          <p>
            Você tem certeza de que deseja mover{" "}
            <strong className="font-mono text-zinc-100">
              {selectedIds.size} imagens
            </strong>{" "}
            para a lixeira? Elas poderão ser restauradas a qualquer momento.
          </p>
        }
        confirmLabel="Mover para a lixeira"
        danger
        busy={batchDeleteBusy}
        onConfirm={onConfirmBatchDelete}
        onClose={() => {
          if (!batchDeleteBusy) setBatchDeleteOpen(false);
        }}
      />

      {/* Confirmação de exclusão individual */}
      <ConfirmDialog
        open={deleting !== null}
        title="Mover para a lixeira"
        body={
          deleting ? (
            <p>
              <strong className="text-zinc-100">{deleting.filename}</strong>{" "}
              vai para a lixeira — restaurável até você esvaziá-la.
            </p>
          ) : null
        }
        confirmLabel="Mover para a lixeira"
        danger
        busy={deleteBusy}
        onConfirm={onConfirmSoftDelete}
        onClose={() => {
          if (!deleteBusy) setDeleting(null);
        }}
      />

      {/* Confirmação de esvaziamento da lixeira */}
      <ConfirmDialog
        open={purgeOpen}
        title="Esvaziar lixeira"
        body={
          <p>
            Exclusão permanente — não há como restaurar depois.{" "}
            <span className="font-mono">
              {trashTotal.toLocaleString()}{" "}
              {trashTotal === 1
                ? "imagem será removida"
                : "imagens serão removidas"}
            </span>{" "}
            para sempre.
          </p>
        }
        confirmLabel="Esvaziar lixeira"
        danger
        busy={purgeBusy}
        onConfirm={onConfirmPurge}
        onClose={() => {
          if (!purgeBusy) setPurgeOpen(false);
        }}
      />

      {classesOpen && (
        <ClassesModal
          datasetId={dataset.id}
          datasetClasses={dataset.classes}
          onClose={() => setClassesOpen(false)}
          onSaved={onClassesSaved}
        />
      )}

      {importOpen && (
        <CreateDatasetModal
          open={importOpen}
          initialMode="import"
          onClose={() => setImportOpen(false)}
          onCreated={(created) => router.push(`/datasets/${created.id}`)}
        />
      )}

      {trainOpen && (
        <TrainYoloModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          onClose={() => setTrainOpen(false)}
          onJobCreated={() => setTrainOpen(false)}
        />
      )}

      {autoTrackerOpen && (
        <AutoTrackerModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          onClose={() => setAutoTrackerOpen(false)}
          onJobCreated={() => setAutoTrackerOpen(false)}
        />
      )}

      {autoLabelOpen && (
        <AutoLabelModal
          open
          datasetId={dataset.id}
          datasetTitle={dataset.title}
          classes={dataset.classes ?? []}
          selectedImageIds={Array.from(selectedIds)}
          totalImagesCount={dataset.imagesCount}
          onClose={() => setAutoLabelOpen(false)}
          onJobCreated={() => setAutoLabelOpen(false)}
        />
      )}

      {batchEditClassesOpen && (
        <BatchEditClassesModal
          open
          datasetId={dataset.id}
          datasetClasses={dataset.classes ?? []}
          selectedImageIds={Array.from(selectedIds)}
          totalInView={items.length}
          onClose={() => setBatchEditClassesOpen(false)}
          onSuccess={onRefresh}
          onClassesUpdated={(newClasses) => onClassesSaved(newClasses)}
        />
      )}
    </>
  );
}
