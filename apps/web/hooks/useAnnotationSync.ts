"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { putBoxes } from "@/lib/images";
import type { BBoxData, BoxInput } from "@/types/studio";

export interface UseAnnotationSyncOptions {
  datasetId: string;
  imageId: string;
  initialBoxes?: BBoxData[];
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

export function useAnnotationSync({
  datasetId,
  imageId,
  initialBoxes = [],
}: UseAnnotationSyncOptions) {
  const [boxes, setBoxes] = useState<BBoxData[]>(initialBoxes);
  const [selectedBoxId, setSelectedBoxId] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  const boxesRef = useRef<BBoxData[]>(boxes);
  boxesRef.current = boxes;
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  const savingRef = useRef(saving);
  savingRef.current = saving;
  const selectedBoxIdRef = useRef<string | null>(selectedBoxId);
  selectedBoxIdRef.current = selectedBoxId;

  const saveTimeoutRef = useRef<number | null>(null);
  const mutationCountRef = useRef(0);

  // Re-inicializa caixas quando o initialBoxes mudar
  useEffect(() => {
    setBoxes(initialBoxes.map((b) => ({ ...b })));
    setDirty(false);
    setSelectedBoxId(null);
  }, [initialBoxes]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: contador intencional de mutações
  useEffect(() => {
    mutationCountRef.current += 1;
  }, [boxes]);

  const handleSave = useCallback(async () => {
    if (savingRef.current) {
      if (saveTimeoutRef.current !== null) {
        window.clearTimeout(saveTimeoutRef.current);
      }
      saveTimeoutRef.current = window.setTimeout(() => {
        saveTimeoutRef.current = null;
        void handleSaveRef.current();
      }, 400);
      return;
    }

    const current = boxesRef.current;
    const snapshotCounter = mutationCountRef.current;

    if (current.length > 1000) {
      showToast("Máximo de 1000 caixas.", "error");
      return;
    }

    if (saveTimeoutRef.current !== null) {
      window.clearTimeout(saveTimeoutRef.current);
      saveTimeoutRef.current = null;
    }

    setSaving(true);
    try {
      const payload: BoxInput[] = current.map((b) => {
        const base: BoxInput = {
          classId: b.classId,
          x: clamp01(b.x),
          y: clamp01(b.y),
          w: clamp01(b.w),
          h: clamp01(b.h),
        };
        if (b.conf !== null && b.conf !== undefined) base.conf = b.conf;
        if (b.origin) base.origin = b.origin;
        if (b.trackId !== null && b.trackId !== undefined)
          base.trackId = b.trackId;
        return base;
      });

      const res = await putBoxes(datasetId, imageId, payload);

      if (mutationCountRef.current !== snapshotCounter) {
        setDirty(true);
        if (saveTimeoutRef.current !== null) {
          window.clearTimeout(saveTimeoutRef.current);
        }
        saveTimeoutRef.current = window.setTimeout(() => {
          saveTimeoutRef.current = null;
          void handleSaveRef.current();
        }, 400);
        return;
      }

      const idxSel = current.findIndex(
        (b) => b.id === selectedBoxIdRef.current,
      );
      setBoxes(res.boxes.map((b) => ({ ...b })));
      setSelectedBoxId(
        idxSel >= 0 && res.boxes[idxSel] ? res.boxes[idxSel].id : null,
      );
      setDirty(false);
      showToast("Anotações salvas.", "success");
    } catch (err) {
      const message =
        err instanceof ApiError && err.message
          ? err.message
          : "Falha ao salvar anotações.";
      showToast(message, "error");
    } finally {
      setSaving(false);
    }
  }, [datasetId, imageId]);

  const handleSaveRef = useRef(handleSave);
  handleSaveRef.current = handleSave;

  const requestSave = useCallback(() => {
    if (saveTimeoutRef.current !== null) {
      window.clearTimeout(saveTimeoutRef.current);
    }
    saveTimeoutRef.current = window.setTimeout(() => {
      saveTimeoutRef.current = null;
      void handleSaveRef.current();
    }, 800);
  }, []);

  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current !== null) {
        window.clearTimeout(saveTimeoutRef.current);
      }
    };
  }, []);

  return {
    boxes,
    setBoxes,
    selectedBoxId,
    setSelectedBoxId,
    dirty,
    setDirty,
    saving,
    handleSave,
    requestSave,
    boxesRef,
    dirtyRef,
    selectedBoxIdRef,
  };
}
