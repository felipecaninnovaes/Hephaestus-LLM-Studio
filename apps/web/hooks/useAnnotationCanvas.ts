"use client";

import {
  type Dispatch,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { showToast } from "@/components/ui/Toast";
import { newId } from "@/lib/id";
import type { BBoxData, StudioClass } from "@/types/studio";

export type ToolId = "bbox" | "select" | "pan";

export interface UseAnnotationCanvasOptions {
  classes: StudioClass[];
  selectedClassId: string;
  setSelectedClassId: (id: string) => void;
  setBoxes: Dispatch<SetStateAction<BBoxData[]>>;
  selectedBoxId: string | null;
  setSelectedBoxId: (id: string | null) => void;
  setDirty: (dirty: boolean) => void;
  dirtyRef: RefObject<boolean>;
  selectedBoxIdRef: RefObject<string | null>;
  requestSave: () => void;
  classesOpen: boolean;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

export function useAnnotationCanvas({
  classes,
  selectedClassId,
  setSelectedClassId,
  setBoxes,
  setSelectedBoxId,
  setDirty,
  dirtyRef,
  selectedBoxIdRef,
  requestSave,
  classesOpen,
}: UseAnnotationCanvasOptions) {
  const [activeTool, setActiveTool] = useState<ToolId>("bbox");
  const [zoom, setZoom] = useState(100);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [draft, setDraft] = useState<BBoxData | null>(null);

  const frameRef = useRef<HTMLDivElement | null>(null);
  const activeToolRef = useRef<ToolId>(activeTool);
  activeToolRef.current = activeTool;
  const classesRef = useRef<StudioClass[]>(classes);
  classesRef.current = classes;
  const selectedClassIdRef = useRef(selectedClassId);
  selectedClassIdRef.current = selectedClassId;
  const classesOpenRef = useRef(classesOpen);
  classesOpenRef.current = classesOpen;

  const drawRef = useRef<{ startX: number; startY: number } | null>(null);
  const moveRef = useRef<{ id: string; offX: number; offY: number } | null>(
    null,
  );
  const resizeRef = useRef<{ id: string } | null>(null);
  const panRef = useRef<{
    sx: number;
    sy: number;
    ox: number;
    oy: number;
  } | null>(null);

  const toNorm = useCallback((clientX: number, clientY: number) => {
    const el = frameRef.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: clamp01((clientX - rect.left) / rect.width),
      y: clamp01((clientY - rect.top) / rect.height),
      mw: rect.width,
      mh: rect.height,
    };
  }, []);

  // Movimento global (desenho, mover, resize, pan) via window
  useEffect(() => {
    function onMove(e: MouseEvent) {
      const frame = frameRef.current;
      if (!frame) return;
      const rect = frame.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) return;
      const nx = clamp01((e.clientX - rect.left) / rect.width);
      const ny = clamp01((e.clientY - rect.top) / rect.height);

      if (drawRef.current) {
        const { startX, startY } = drawRef.current;
        setDraft({
          id: "__draft__",
          classId: "",
          x: Math.min(startX, nx),
          y: Math.min(startY, ny),
          w: Math.abs(nx - startX),
          h: Math.abs(ny - startY),
          conf: null,
          origin: "",
          trackId: null,
        });
        return;
      }
      if (moveRef.current) {
        const { id: bid, offX, offY } = moveRef.current;
        setBoxes((prev) =>
          prev.map((b) =>
            b.id === bid
              ? { ...b, x: clamp01(nx - offX), y: clamp01(ny - offY) }
              : b,
          ),
        );
        return;
      }
      if (resizeRef.current) {
        const { id: bid } = resizeRef.current;
        setBoxes((prev) =>
          prev.map((b) => {
            if (b.id !== bid) return b;
            const minW = 2 / rect.width;
            const minH = 2 / rect.height;
            const rawW = nx - b.x;
            const rawH = ny - b.y;
            const w = Math.min(1 - b.x, rawW < minW ? minW : clamp01(rawW));
            const h = Math.min(1 - b.y, rawH < minH ? minH : clamp01(rawH));
            return { ...b, w, h };
          }),
        );
        return;
      }
      if (panRef.current) {
        const p = panRef.current;
        setPan((prev) => {
          const limX = 2 * rect.width;
          const limY = 2 * rect.height;
          const cx = Math.min(
            limX,
            Math.max(-limX, p.ox + (e.clientX - p.sx)),
          );
          const cy = Math.min(
            limY,
            Math.max(-limY, p.oy + (e.clientY - p.sy)),
          );
          return prev.x === cx && prev.y === cy ? prev : { x: cx, y: cy };
        });
      }
    }

    function onUp(e: MouseEvent) {
      const frame = frameRef.current;
      if (drawRef.current && frame) {
        const rect = frame.getBoundingClientRect();
        const { startX, startY } = drawRef.current;
        drawRef.current = null;
        const nx = clamp01((e.clientX - rect.left) / rect.width);
        const ny = clamp01((e.clientY - rect.top) / rect.height);
        const x = Math.min(startX, nx);
        const y = Math.min(startY, ny);
        const w = Math.abs(nx - startX);
        const h = Math.abs(ny - startY);
        setDraft(null);
        if (w * rect.width < 2 || h * rect.height < 2) return;
        const activeCls = classesRef.current.find(
          (c) => c.id === selectedClassIdRef.current,
        );
        const classId =
          activeCls?.id ?? classesRef.current[0]?.id ?? "";
        if (!classId) return;
        const nid = newId();
        setBoxes((prev) => [
          ...prev,
          {
            id: nid,
            classId,
            x,
            y,
            w,
            h,
            conf: null,
            origin: "",
            trackId: null,
          },
        ]);
        setSelectedBoxId(nid);
        setDirty(true);
        requestSave();
        return;
      }
      if (moveRef.current) {
        moveRef.current = null;
        setDirty(true);
        requestSave();
        return;
      }
      if (resizeRef.current) {
        resizeRef.current = null;
        setDirty(true);
        requestSave();
      }
      if (panRef.current) panRef.current = null;
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [setBoxes, setSelectedBoxId, setDirty, requestSave]);

  function onFrameMouseDown(e: ReactMouseEvent) {
    e.preventDefault();
    const tool = activeToolRef.current;
    if (tool === "pan") {
      panRef.current = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
      return;
    }
    if (tool === "bbox") {
      if (!classesRef.current.length) {
        showToast(
          "Este dataset não tem classes — crie uma antes de desenhar caixas.",
          "error",
        );
        return;
      }
      const p = toNorm(e.clientX, e.clientY);
      if (!p) return;
      drawRef.current = { startX: p.x, startY: p.y };
    }
  }

  function onBoxMouseDown(e: ReactMouseEvent, box: BBoxData) {
    e.stopPropagation();
    e.preventDefault();
    setSelectedBoxId(box.id);
    const t = e.target as HTMLElement | null;
    if (t?.closest?.("[data-resize-handle]")) {
      resizeRef.current = { id: box.id };
      return;
    }
    if (activeToolRef.current !== "select") return;
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    moveRef.current = { id: box.id, offX: p.x - box.x, offY: p.y - box.y };
  }

  // Atalhos de teclado
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (classesOpenRef.current) return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.tagName === "SELECT" ||
          t.isContentEditable)
      )
        return;
      const k = e.key;
      if (k === "b" || k === "B") {
        setActiveTool("bbox");
        return;
      }
      if (k === "v" || k === "V") {
        setActiveTool("select");
        return;
      }
      if (k === "h" || k === "H") {
        setActiveTool("pan");
        return;
      }
      if (k === "Escape") {
        if (drawRef.current) {
          drawRef.current = null;
          setDraft(null);
        } else {
          setSelectedBoxId(null);
        }
        return;
      }
      if (k === "Delete" || k === "Backspace") {
        const sel = selectedBoxIdRef.current;
        if (!sel) return;
        e.preventDefault();
        setBoxes((prev) => prev.filter((b) => b.id !== sel));
        setSelectedBoxId(null);
        setDirty(true);
        requestSave();
        return;
      }
      if (/^Digit[1-9]$/.test(e.code)) {
        const idx = Number(e.code.slice(5)) - 1;
        const cls = classesRef.current[idx];
        if (!cls) return;
        setSelectedClassId(cls.id);
        const sel = selectedBoxIdRef.current;
        if (sel) {
          setBoxes((prev) =>
            prev.map((b) => (b.id === sel ? { ...b, classId: cls.id } : b)),
          );
          setDirty(true);
          requestSave();
        }
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setBoxes, setSelectedBoxId, setSelectedClassId, setDirty, requestSave, selectedBoxIdRef]);

  // Proteção de saída ao sair com alterações não salvas
  useEffect(() => {
    function onBeforeUnload(e: BeforeUnloadEvent) {
      if (!dirtyRef.current) return;
      e.preventDefault();
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [dirtyRef]);

  return {
    frameRef,
    zoom,
    setZoom,
    pan,
    setPan,
    activeTool,
    setActiveTool,
    draft,
    onFrameMouseDown,
    onBoxMouseDown,
  };
}
