"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { showToast } from "@/components/ui/Toast";
import ClassesModal from "@/components/studio/ClassesModal";
import {
  AnnotationCanvasView,
  AnnotationSidebar,
} from "@/components/studio/annotation";
import { useAnnotationCanvas } from "@/hooks/useAnnotationCanvas";
import { useAnnotationSync } from "@/hooks/useAnnotationSync";
import { ApiError } from "@/lib/api";
import { getDataset } from "@/lib/datasets";
import { getImage } from "@/lib/images";
import type { Dataset, ImageDetail, StudioClass } from "@/types/studio";

export default function AnnotateImagePage() {
  const params = useParams<{ id: string; imageId: string }>();
  const id = params?.id ?? "";
  const imageId = params?.imageId ?? "";
  const router = useRouter();

  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [detail, setDetail] = useState<ImageDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedClassId, setSelectedClassId] = useState<string>("");
  const [classesOpen, setClassesOpen] = useState(false);

  const {
    boxes,
    setBoxes,
    selectedBoxId,
    setSelectedBoxId,
    dirty,
    setDirty,
    saving,
    handleSave,
    requestSave,
    dirtyRef,
    selectedBoxIdRef,
  } = useAnnotationSync({
    datasetId: id,
    imageId,
    initialBoxes: detail?.boxes,
  });

  const classes = useMemo(
    () => (dataset ? [...dataset.classes].sort((a, b) => a.idx - b.idx) : []),
    [dataset],
  );

  const {
    frameRef,
    zoom,
    setZoom,
    pan,
    activeTool,
    setActiveTool,
    draft,
    onFrameMouseDown,
    onBoxMouseDown,
  } = useAnnotationCanvas({
    classes,
    selectedClassId,
    setSelectedClassId,
    setBoxes,
    selectedBoxId,
    setSelectedBoxId,
    setDirty,
    dirtyRef,
    selectedBoxIdRef,
    requestSave,
    classesOpen,
  });

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [ds, img] = await Promise.all([
        getDataset(id),
        getImage(id, imageId),
      ]);
      if (ds.category !== "yolo") {
        showToast("O editor de caixas é só para datasets YOLO.", "error");
        router.push(`/datasets/${id}`);
        return;
      }
      setDataset(ds);
      setDetail(img);
      const first = [...ds.classes].sort((a, b) => a.idx - b.idx)[0];
      setSelectedClassId(first ? first.id : "");
    } catch (err) {
      if (
        err instanceof ApiError &&
        (err.code === "unauthorized" || err.status === 401)
      ) {
        router.replace("/login");
        return;
      }
      const message =
        err instanceof ApiError && err.status === 404
          ? "Dataset ou imagem não encontrado."
          : "Falha ao carregar o editor.";
      showToast(message, "error");
      router.push(`/datasets/${id}`);
    } finally {
      setLoading(false);
    }
  }, [id, imageId, router]);

  useEffect(() => {
    if (id && imageId) void load();
  }, [id, imageId, load]);

  useEffect(() => {
    function handleDatasetUpdated(e: Event) {
      const ce = e as CustomEvent<{ datasetId?: string }>;
      if (!ce.detail || ce.detail.datasetId === id) {
        void load();
      }
    }
    window.addEventListener("hephaestus:dataset-updated", handleDatasetUpdated);
    return () => {
      window.removeEventListener(
        "hephaestus:dataset-updated",
        handleDatasetUpdated,
      );
    };
  }, [id, load]);

  const selectedBox = useMemo(
    () => boxes.find((b) => b.id === selectedBoxId) ?? null,
    [boxes, selectedBoxId],
  );

  const classById = useMemo(() => {
    const map = new Map<string, { name: string; color: string }>();
    for (const c of classes) map.set(c.id, { name: c.name, color: c.color });
    return map;
  }, [classes]);

  const frameWidth = 600 * (zoom / 100);
  const frameHeight = useMemo(() => {
    if (
      detail &&
      Number.isFinite(detail.width) &&
      Number.isFinite(detail.height) &&
      detail.width > 0 &&
      detail.height > 0
    ) {
      return frameWidth * (detail.height / detail.width);
    }
    return 450 * (zoom / 100);
  }, [detail, frameWidth, zoom]);

  async function handleClassesSaved(newClasses: StudioClass[]) {
    const ordered = [...newClasses].sort((a, b) => a.idx - b.idx);
    setDataset((prev) => (prev ? { ...prev, classes: newClasses } : prev));
    setSelectedClassId((prev) =>
      ordered.some((c) => c.id === prev) ? prev : (ordered[0]?.id ?? ""),
    );
    try {
      const fresh = await getDataset(id);
      setDataset(fresh);
      const freshOrdered = [...fresh.classes].sort((a, b) => a.idx - b.idx);
      setSelectedClassId((prev) =>
        freshOrdered.some((c) => c.id === prev)
          ? prev
          : (freshOrdered[0]?.id ?? ""),
      );
    } catch {
      // Mantém atualização otimista
    }
  }

  if (loading || !dataset || !detail) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <p className="py-10 text-center text-sm text-zinc-500">Carregando…</p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-y-auto md:flex-row md:overflow-hidden">
      <AnnotationSidebar
        dataset={dataset}
        classes={classes}
        activeTool={activeTool}
        setActiveTool={setActiveTool}
        selectedClassId={selectedClassId}
        setSelectedClassId={setSelectedClassId}
        selectedBox={selectedBox}
        saving={saving}
        dirty={dirty}
        onSave={() => void handleSave()}
        onOpenClasses={() => setClassesOpen(true)}
      />

      <AnnotationCanvasView
        detail={detail}
        frameRef={frameRef}
        activeTool={activeTool}
        zoom={zoom}
        setZoom={setZoom}
        pan={pan}
        frameWidth={frameWidth}
        frameHeight={frameHeight}
        boxes={boxes}
        selectedBoxId={selectedBoxId}
        setSelectedBoxId={setSelectedBoxId}
        draft={draft}
        selectedClassId={selectedClassId}
        classById={classById}
        onFrameMouseDown={onFrameMouseDown}
        onBoxMouseDown={onBoxMouseDown}
      />

      {classesOpen && (
        <ClassesModal
          datasetId={id}
          datasetClasses={dataset.classes}
          onClose={() => setClassesOpen(false)}
          onSaved={handleClassesSaved}
        />
      )}
    </div>
  );
}
