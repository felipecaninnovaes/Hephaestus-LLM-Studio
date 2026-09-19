"use client";

import { useParams, useRouter } from "next/navigation";
import { useState } from "react";
import {
  DatasetDetailHeader,
  DatasetImageGrid,
  DatasetModals,
  DatasetToolbar,
} from "@/components/studio/dataset-detail";
import FloatingSelectionBar from "@/components/studio/FloatingSelectionBar";
import UploadFloatingDock from "@/components/studio/UploadFloatingDock";
import { Button, DropOverlay, useFileDrop } from "@/components/ui";
import { useDatasetGallery } from "@/hooks/useDatasetGallery";
import { useDatasetUpload } from "@/hooks/useDatasetUpload";
import { ApiError } from "@/lib/api";
import { exportDataset, exportErrorMessage } from "@/lib/backup";
import { extractFilesFromDataTransfer } from "@/lib/dataset-inspector";
import type { ImageItem } from "@/types/studio";
import { showToast } from "@/components/ui/Toast";

export default function DatasetGalleryPage() {
  const params = useParams<{ id?: string }>();
  const datasetId = params?.id;
  const router = useRouter();

  const gallery = useDatasetGallery({ datasetId });
  const {
    dataset,
    items,
    setItems,
    total,
    trashTotal,
    loading,
    loadingMore,
    error,
    load,
    sentinelRef,
    view,
    splitView,
    annotationFilter,
    density,
    setDensity,
    selectedClassId,
    handleSplitChange,
    handleAnnotationChange,
    clearAllFilters,
    searchMode,
    setSearchMode,
    searchInput,
    setSearchInput,
    activeTag,
    activeQuery,
    similarFor,
    results,
    searching,
    searchStatus,
    indexBusy,
    clearSearch,
    handleTextSearch,
    handleTriggerIndex,
    selectionMode,
    selectedIds,
    handleSelectToggle,
    handleSelectAll,
    handleClearSelection,
    isAllSelected,
    batchDeleteOpen,
    setBatchDeleteOpen,
    batchDeleteBusy,
    handleConfirmBatchDelete,
    deleting,
    setDeleting,
    deleteBusy,
    handleSoftDelete,
    restoringId,
    handleRestore,
    purgeOpen,
    setPurgeOpen,
    purgeBusy,
    handlePurgeTrash,
    quickLookIndex,
    setQuickLookIndex,
    handleOpenQuickLook,
    handleClassesSaved,
  } = gallery;

  const upload = useDatasetUpload({
    datasetId,
    onSuccess: async () => {
      if (datasetId) {
        await load(datasetId, splitView, annotationFilter);
      }
    },
  });

  const {
    fileRef,
    uploading,
    uploadCount,
    uploadSent,
    uploadBatchInfo,
    auditModalOpen,
    setAuditModalOpen,
    lastUploadResults,
    setLastUploadResults,
    handleFiles,
    cancelUpload,
  } = upload;

  const [classesOpen, setClassesOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [trainOpen, setTrainOpen] = useState(false);
  const [autoTrackerOpen, setAutoTrackerOpen] = useState(false);
  const [autoLabelOpen, setAutoLabelOpen] = useState(false);
  const [batchEditClassesOpen, setBatchEditClassesOpen] = useState(false);
  const [exporting, setExporting] = useState(false);

  const { isDragging: isDraggingPage, dropProps } = useFileDrop({
    onDropFiles: async (files: FileList | File[], dataTransfer?: DataTransfer) => {
      if (dataTransfer) {
        try {
          const extracted = await extractFilesFromDataTransfer(dataTransfer);
          if (extracted.length > 0) {
            await handleFiles(extracted);
            return;
          }
        } catch {
          // fallback para arquivos diretos
        }
      }
      await handleFiles(files);
    },
  });

  async function handleExport() {
    if (!dataset || exporting) return;
    setExporting(true);
    try {
      await exportDataset(dataset.id, dataset.slug);
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      const code = err instanceof ApiError ? err.code : "export_failed";
      showToast(exportErrorMessage(code), "error");
    } finally {
      setExporting(false);
    }
  }

  function handleTileClick(item: ImageItem) {
    if (!dataset || view === "trash") return;
    if (dataset.category === "yolo") {
      router.push(`/datasets/${datasetId}/annotate/${item.id}`);
    } else {
      handleOpenQuickLook(item);
    }
  }

  if (loading) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <p className="py-10 text-center font-mono text-xs text-zinc-400">
          Carregando galeria…
        </p>
      </div>
    );
  }

  if (error || !dataset) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <Button
          type="button"
          variant="ghost"
          size="md"
          onClick={() => router.push("/datasets")}
          className="w-fit cursor-pointer"
        >
          ← Datasets
        </Button>
        <div className="glass-card flex flex-col items-center gap-3 rounded-2xl p-10 text-center">
          <p className="text-sm text-zinc-300">
            {error ?? "Dataset não encontrado."}
          </p>
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => datasetId && load(datasetId)}
          >
            Tentar novamente
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div
      {...dropProps}
      className="relative mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6"
    >
      <DropOverlay
        open={isDraggingPage}
        title="Solte as imagens aqui"
        subtitle={`Upload direto de amostras para o dataset ${dataset.title}`}
        onDragLeave={dropProps.onDragLeave}
        onDrop={dropProps.onDrop}
      />

      <DatasetDetailHeader
        dataset={dataset}
        trashTotal={trashTotal}
        exporting={exporting}
        onExport={handleExport}
        onOpenClasses={() => setClassesOpen(true)}
        onOpenAutoLabel={() => setAutoLabelOpen(true)}
        onOpenAutoTracker={() => setAutoTrackerOpen(true)}
        onOpenImport={() => setImportOpen(true)}
        onOpenTrain={() => setTrainOpen(true)}
      />

      <DatasetToolbar
        dataset={dataset}
        trashTotal={trashTotal}
        splitView={splitView}
        onSplitChange={handleSplitChange}
        annotationFilter={annotationFilter}
        onAnnotationFilterChange={handleAnnotationChange}
        density={density}
        onDensityChange={setDensity}
        selectionMode={selectionMode}
        onToggleSelectionMode={() => {
          if (selectionMode) {
            handleClearSelection();
          } else {
            handleSelectAll();
          }
        }}
        selectedCount={selectedIds.size}
        isAllSelected={isAllSelected}
        onSelectAll={handleSelectAll}
        onClearSelection={handleClearSelection}
        selectedClassId={selectedClassId}
        onClassChange={(clsId) => {
          if (clsId === null) {
            clearAllFilters();
          } else if (datasetId) {
            void load(
              datasetId,
              splitView,
              annotationFilter,
              clsId,
              activeTag,
            );
          }
        }}
        searchMode={searchMode}
        setSearchMode={setSearchMode}
        searchInput={searchInput}
        setSearchInput={setSearchInput}
        onClearSearch={clearSearch}
        onSearchSubmit={handleTextSearch}
        searching={searching}
        searchStatus={searchStatus}
        indexBusy={indexBusy}
        onTriggerIndex={handleTriggerIndex}
        activeTag={activeTag}
        total={total}
        onClearAllFilters={clearAllFilters}
      />

      <DatasetImageGrid
        dataset={dataset}
        items={items}
        total={total}
        splitView={splitView}
        density={density}
        annotationFilter={annotationFilter}
        selectionMode={selectionMode}
        selectedIds={selectedIds}
        onSelectToggle={handleSelectToggle}
        onSelectAll={handleSelectAll}
        onClearSelection={handleClearSelection}
        onRestore={(item) => void handleRestore(item.id)}
        restoringId={restoringId}
        onDelete={(item) => setDeleting(item)}
        onQuickLook={(item) => handleOpenQuickLook(item)}
        onTileClick={handleTileClick}
        results={results}
        similarFor={similarFor}
        searchMode={searchMode}
        activeQuery={activeQuery}
        activeTag={activeTag}
        selectedClassId={selectedClassId}
        onClearSearch={clearSearch}
        onClearAllFilters={clearAllFilters}
        uploading={uploading}
        uploadSent={uploadSent}
        uploadCount={uploadCount}
        uploadBatchInfo={uploadBatchInfo}
        onUploadClick={() => fileRef.current?.click()}
        onCancelUpload={cancelUpload}
        sentinelRef={sentinelRef}
        loadingMore={loadingMore}
      />

      {selectedIds.size > 0 && (
        <FloatingSelectionBar
          selectedCount={selectedIds.size}
          totalInView={items.length}
          onSelectAll={handleSelectAll}
          onClearSelection={handleClearSelection}
          onBatchDelete={() => setBatchDeleteOpen(true)}
          onAutoLabel={() => setAutoLabelOpen(true)}
          onBatchEditClasses={() => setBatchEditClassesOpen(true)}
          busy={batchDeleteBusy}
        />
      )}

      <UploadFloatingDock
        uploading={uploading}
        uploadSent={uploadSent}
        uploadCount={uploadCount}
        uploadBatchInfo={uploadBatchInfo}
        lastResults={lastUploadResults}
        onCancel={cancelUpload}
        onOpenAudit={() => setAuditModalOpen(true)}
        onDismiss={() => setLastUploadResults(null)}
      />

      <input
        ref={fileRef}
        type="file"
        multiple
        accept="image/*"
        className="hidden"
        onChange={(e) => void handleFiles(e.target.files)}
      />

      <DatasetModals
        dataset={dataset}
        items={items}
        setItems={setItems}
        classesOpen={classesOpen}
        setClassesOpen={setClassesOpen}
        importOpen={importOpen}
        setImportOpen={setImportOpen}
        trainOpen={trainOpen}
        setTrainOpen={setTrainOpen}
        autoTrackerOpen={autoTrackerOpen}
        setAutoTrackerOpen={setAutoTrackerOpen}
        autoLabelOpen={autoLabelOpen}
        setAutoLabelOpen={setAutoLabelOpen}
        batchEditClassesOpen={batchEditClassesOpen}
        setBatchEditClassesOpen={setBatchEditClassesOpen}
        selectedIds={selectedIds}
        auditModalOpen={auditModalOpen}
        setAuditModalOpen={setAuditModalOpen}
        lastUploadResults={lastUploadResults}
        quickLookIndex={quickLookIndex}
        setQuickLookIndex={setQuickLookIndex}
        deleting={deleting}
        setDeleting={setDeleting}
        deleteBusy={deleteBusy}
        onConfirmSoftDelete={() => deleting && void handleSoftDelete(deleting)}
        batchDeleteOpen={batchDeleteOpen}
        setBatchDeleteOpen={setBatchDeleteOpen}
        batchDeleteBusy={batchDeleteBusy}
        onConfirmBatchDelete={() => void handleConfirmBatchDelete()}
        purgeOpen={purgeOpen}
        setPurgeOpen={setPurgeOpen}
        purgeBusy={purgeBusy}
        onConfirmPurge={() => void handlePurgeTrash()}
        trashTotal={trashTotal}
        onClassesSaved={handleClassesSaved}
        onTileClick={handleTileClick}
        onRefresh={() => {
          if (datasetId) {
            void load(datasetId, splitView, annotationFilter, selectedClassId, activeTag);
          }
        }}
      />
    </div>
  );
}
