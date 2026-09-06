"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useRouter } from "next/navigation";
import { showToast } from "@/components/studio/Toast";
import {
  IconBoxSelect,
  IconLayers,
  IconTarget,
  IconZoomIn,
  IconZoomOut,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import { getDataset } from "@/lib/datasets";
import { getImage, putBoxes } from "@/lib/images";
import type {
  BBoxData,
  BoxInput,
  Dataset,
  ImageDetail,
} from "@/types/studio";

type ToolId = "bbox" | "select" | "pan";

const TOOLS: { id: ToolId; label: string }[] = [
  { id: "bbox", label: "Caixa Delimitadora (BBox - B)" },
  { id: "select", label: "Mover / Selecionar (V)" },
  { id: "pan", label: "Arrastar Canvas (H)" },
];

function toolIcon(id: ToolId) {
  if (id === "bbox") return IconBoxSelect;
  if (id === "select") return IconTarget;
  return IconLayers;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

export default function AnnotateImagePage() {
  const { id, imageId } = useParams<{ id: string; imageId: string }>();
  const router = useRouter();

  const [dataset, setDataset] = useState<Dataset | null>(null);
  const [detail, setDetail] = useState<ImageDetail | null>(null);
  const [loading, setLoading] = useState(true);

  const [boxes, setBoxes] = useState<BBoxData[]>([]);
  const [activeTool, setActiveTool] = useState<ToolId>("bbox");
  const [selectedClassId, setSelectedClassId] = useState<string>("");
  const [selectedBoxId, setSelectedBoxId] = useState<string | null>(null);
  const [zoom, setZoom] = useState(100);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [draft, setDraft] = useState<BBoxData | null>(null);

  const frameRef = useRef<HTMLDivElement | null>(null);
  const boxesRef = useRef<BBoxData[]>([]);
  boxesRef.current = boxes;
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;
  const savingRef = useRef(false);
  savingRef.current = saving;
  const classesRef = useRef<Dataset["classes"]>([]);
  const selectedBoxIdRef = useRef<string | null>(null);
  selectedBoxIdRef.current = selectedBoxId;
  const activeToolRef = useRef<ToolId>("bbox");
  activeToolRef.current = activeTool;
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const drawRef = useRef<{ startX: number; startY: number } | null>(null);
  const moveRef = useRef<{ id: string; offX: number; offY: number } | null>(null);
  const resizeRef = useRef<{ id: string } | null>(null);
  const panRef = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [ds, img] = await Promise.all([getDataset(id), getImage(id, imageId)]);
      if (ds.category !== "yolo") {
        showToast("O editor de caixas é só para datasets YOLO.", "error");
        router.push(`/datasets/${id}`);
        return;
      }
      setDataset(ds);
      setDetail(img);
      setBoxes(img.boxes.map((b) => ({ ...b })));
      const first = [...ds.classes].sort((a, b) => a.idx - b.idx)[0];
      setSelectedClassId(first ? first.id : "");
      setSelectedBoxId(null);
      setDirty(false);
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
    if (id && imageId) load();
  }, [id, imageId, load]);

  const classes = useMemo(
    () => (dataset ? [...dataset.classes].sort((a, b) => a.idx - b.idx) : []),
    [dataset],
  );

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

  async function handleSave() {
    if (savingRef.current) return;
    const current = boxesRef.current;
    if (current.length > 1000) {
      showToast("Máximo de 1000 caixas.", "error");
      return;
    }
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
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
        // Preserva metadados das caixas vindas do backend (ex.: origin='autotracker');
        // caixas novas (sem id do backend) omitem os 3 campos e o backend defaulta origin=manual.
        if (b.conf !== null && b.conf !== undefined) base.conf = b.conf;
        if (b.origin) base.origin = b.origin;
        if (b.trackId !== null && b.trackId !== undefined) base.trackId = b.trackId;
        return base;
      });
      const res = await putBoxes(id, imageId, payload);
      setBoxes(res.boxes.map((b) => ({ ...b })));
      setSelectedBoxId((prev) =>
        prev && res.boxes.some((b) => b.id === prev) ? prev : null,
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
  }

  const requestSave = useCallback(() => {
    if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    saveTimeoutRef.current = setTimeout(() => {
      saveTimeoutRef.current = null;
      void handleSaveRef.current();
    }, 800);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSaveRef = useRef(handleSave);
  handleSaveRef.current = handleSave;

  useEffect(() => {
    classesRef.current = classes;
  }, [classes]);

  // 1. Conversão px↔norm (rect recalculado a cada evento — imune ao zoom).
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

  // 2/3/4. Movimento global (desenho, mover, resize, pan) via window.
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
          const cx = Math.min(limX, Math.max(-limX, p.ox + (e.clientX - p.sx)));
          const cy = Math.min(limY, Math.max(-limY, p.oy + (e.clientY - p.sy)));
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
        if (w * rect.width < 2 || h * rect.height < 2) return; // descarta rascunho mínimo
        const activeCls = classesRef.current.find(
          (c) => c.id === selectedClassIdRef.current,
        );
        const classId =
          activeCls?.id ?? classesRef.current[0]?.id ?? "";
        if (!classId) return;
        const nid = crypto.randomUUID();
        setBoxes((prev) => [
          ...prev,
          { id: nid, classId, x, y, w, h, conf: null, origin: "", trackId: null },
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
      if (panRef.current) panRef.current = null; // pan não seleciona nem suja
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestSave]);

  const selectedClassIdRef = useRef(selectedClassId);
  selectedClassIdRef.current = selectedClassId;

  function onFrameMouseDown(e: React.MouseEvent) {
    e.preventDefault();
    const tool = activeToolRef.current;
    if (tool === "pan") {
      panRef.current = { sx: e.clientX, sy: e.clientY, ox: pan.x, oy: pan.y };
      return;
    }
    if (tool === "bbox") {
      const p = toNorm(e.clientX, e.clientY);
      if (!p) return;
      drawRef.current = { startX: p.x, startY: p.y };
    }
    // tool select sobre vazio: desseleciona (via onClick do frame)
  }

  function onBoxMouseDown(e: React.MouseEvent, box: BBoxData) {
    e.stopPropagation();
    e.preventDefault();
    setSelectedBoxId(box.id);
    if (activeToolRef.current !== "select") return; // bbox: só seleciona
    const p = toNorm(e.clientX, e.clientY);
    if (!p) return;
    moveRef.current = { id: box.id, offX: p.x - box.x, offY: p.y - box.y };
  }

  function onResizeMouseDown(e: React.MouseEvent, box: BBoxData) {
    e.stopPropagation();
    e.preventDefault();
    setSelectedBoxId(box.id);
    resizeRef.current = { id: box.id };
  }

  // 5. Atalhos de teclado.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable)) return;
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
  }, [requestSave]);

  // 6b. Proteção de saída (beforeunload só avisa; PUT não acontece no unload).
  useEffect(() => {
    function onBeforeUnload(e: BeforeUnloadEvent) {
      if (!dirtyRef.current) return;
      e.preventDefault();
    }
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, []);

  useEffect(() => {
    return () => {
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
    };
  }, []);

  if (loading || !dataset || !detail) {
    return (
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
        <p className="py-10 text-center text-sm text-zinc-500">Carregando…</p>
      </div>
    );
  }

  const hasClasses = classes.length > 0;

  return (
    <div className="flex flex-1 flex-col overflow-y-auto md:flex-row md:overflow-hidden">
      {/* Ferramentas e Classes Laterais */}
      <div className="w-full space-y-5 border-r border-zinc-800/80 bg-zinc-950/60 p-4 md:w-72 md:overflow-y-auto">
        <button
          type="button"
          onClick={() => router.push(`/datasets/${id}`)}
          className="flex w-full items-center space-x-2 rounded-xl border border-zinc-800 bg-zinc-900 px-3 py-2 text-xs font-medium text-zinc-300 transition-colors hover:bg-zinc-800 hover:text-white"
        >
          <svg
            className="h-3.5 w-3.5"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.7"
            viewBox="0 0 24 24"
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
          <span>Voltar para a galeria · {dataset.title}</span>
        </button>

        <div>
          <h3 className="text-xs font-semibold tracking-caps text-zinc-200 uppercase">
            Ferramentas de Anotação
          </h3>
          <div className="mt-2 space-y-1 text-xs">
            {TOOLS.map((tool) => {
              const ToolIcon = toolIcon(tool.id);
              const isActive = activeTool === tool.id;
              return (
                <button
                  key={tool.id}
                  type="button"
                  onClick={() => setActiveTool(tool.id)}
                  className={`flex w-full items-center space-x-2 rounded-xl px-3 py-2 text-left font-medium transition-all ${
                    isActive
                      ? "border border-emerald-500/30 bg-emerald-500/15 text-emerald-300"
                      : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200"
                  }`}
                >
                  <ToolIcon />
                  <span>{tool.label}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="border-t border-zinc-800 pt-4">
          <span className="mb-2 block text-xs font-semibold text-zinc-300">
            Classes do Dataset Ativo
          </span>
          {hasClasses ? (
            <div className="space-y-1.5 font-mono text-xs">
              {classes.map((cls) => (
                <button
                  key={cls.id}
                  type="button"
                  onClick={() => setSelectedClassId(cls.id)}
                  className={`flex w-full items-center justify-between rounded-xl p-2 transition-all ${
                    selectedClassId === cls.id
                      ? "border border-zinc-700 bg-zinc-800 text-white"
                      : "text-zinc-400 hover:bg-zinc-900/60"
                  }`}
                >
                  <div className="flex items-center space-x-2">
                    <span
                      className="h-2.5 w-2.5 rounded-full"
                      style={{ background: cls.color }}
                    ></span>
                    <span>
                      {cls.idx}: {cls.name}
                    </span>
                  </div>
                  <span className="text-[10px] text-zinc-400">[{cls.idx + 1}]</span>
                </button>
              ))}
            </div>
          ) : (
            <p className="text-xs text-zinc-400">
              Este dataset ainda não tem classes — crie-as no painel de criação.
            </p>
          )}
        </div>

        <div className="border-t border-zinc-800 pt-4 font-mono text-xs text-zinc-400">
          <span className="tracking-caps mb-2 block text-[10px] text-zinc-400 uppercase">
            Coordenadas YOLO (Norm.)
          </span>
          <div className="space-y-1 rounded-xl border border-zinc-800 bg-zinc-900 p-2.5 text-[11px]">
            <div>
              X:{" "}
              <span className="text-zinc-200">
                {selectedBox ? selectedBox.x.toFixed(6) : "—"}
              </span>
            </div>
            <div>
              Y:{" "}
              <span className="text-zinc-200">
                {selectedBox ? selectedBox.y.toFixed(6) : "—"}
              </span>
            </div>
            <div>
              Width:{" "}
              <span className="text-zinc-200">
                {selectedBox ? selectedBox.w.toFixed(6) : "—"}
              </span>
            </div>
            <div>
              Height:{" "}
              <span className="text-zinc-200">
                {selectedBox ? selectedBox.h.toFixed(6) : "—"}
              </span>
            </div>
          </div>

          <button
            type="button"
            onClick={handleSave}
            disabled={saving || !hasClasses}
            className="mt-4 w-full rounded-xl bg-emerald-500 px-3 py-2 text-xs font-semibold text-zinc-950 transition-colors hover:bg-emerald-400"
          >
            {saving ? "Salvando…" : `${dirty ? "● " : ""}Salvar Anotações`}
          </button>
        </div>
      </div>

      {/* Canvas de Edição */}
      <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center overflow-auto bg-[#0b0f17] p-6">
        <div className="glass-panel absolute top-4 left-6 z-20 flex items-center space-x-2 rounded-xl border border-zinc-800 bg-zinc-900/90 px-3 py-1.5 font-mono text-xs">
          <button
            type="button"
            onClick={() => setZoom((z) => Math.max(50, z - 25))}
            aria-label="Diminuir zoom do canvas"
            className="p-1 text-zinc-400 hover:text-white"
            title="Diminuir Zoom"
          >
            <IconZoomOut />
          </button>
          <span className="min-w-[45px] text-center text-zinc-300">{zoom}%</span>
          <button
            type="button"
            onClick={() => setZoom((z) => Math.min(250, z + 25))}
            aria-label="Aumentar zoom do canvas"
            className="p-1 text-zinc-400 hover:text-white"
            title="Aumentar Zoom"
          >
            <IconZoomIn />
          </button>
          <div className="h-3 w-px bg-zinc-700"></div>
          <button
            type="button"
            onClick={() => setZoom(100)}
            className="text-[10px] text-emerald-400 hover:underline"
          >
            Resetar 100%
          </button>
        </div>

        <div
          ref={frameRef}
          className={`relative flex items-center justify-center overflow-hidden rounded-2xl border-2 border-zinc-700/80 bg-zinc-900/90 shadow-2xl transition-transform duration-200 ${
            activeTool === "pan"
              ? "cursor-grab active:cursor-grabbing"
              : activeTool === "bbox"
                ? "cursor-crosshair"
                : ""
          }`}
          style={{
            width: `${frameWidth}px`,
            height: `${frameHeight}px`,
            transform: `translate(${pan.x}px, ${pan.y}px)`,
          }}
          onMouseDown={onFrameMouseDown}
          onClick={() => {
            if (activeTool !== "pan") setSelectedBoxId(null);
          }}
        >
          <div className="absolute inset-0 bg-[radial-gradient(#ffffff_1px,transparent_1px)] opacity-20 [background-size:18px_18px]"></div>
          <img
            src={detail.url}
            alt={detail.filename}
            className="absolute inset-0 h-full w-full"
            draggable={false}
            style={{ userSelect: "none" }}
          />
          {boxes.map((box, i) => {
            const cls = classById.get(box.classId);
            const color = cls?.color ?? "#71717a";
            const name = cls?.name ?? "classe";
            const isSelected = selectedBoxId === box.id;
            return (
              <div
                key={box.id}
                onMouseDown={(e) => onBoxMouseDown(e, box)}
                onClick={(e) => {
                  e.stopPropagation();
                  setSelectedBoxId(box.id);
                }}
                className={`absolute cursor-move rounded border-2 transition-all ${
                  isSelected ? "shadow-lg ring-2 ring-white/50" : ""
                }`}
                style={{
                  left: `${box.x * 100}%`,
                  top: `${box.y * 100}%`,
                  width: `${box.w * 100}%`,
                  height: `${box.h * 100}%`,
                  borderColor: color,
                  background: color + "26",
                }}
              >
                <span
                  className="pointer-events-none absolute -top-5 left-0 rounded px-1.5 py-0.5 font-mono text-[10px] font-bold"
                  style={{ background: color, color: "#09090b" }}
                >
                  {name} #{i}
                </span>
                {isSelected && (
                  <span
                    onMouseDown={(e) => onResizeMouseDown(e, box)}
                    onClick={(e) => e.stopPropagation()}
                    className="absolute -right-1.5 -bottom-1.5 h-3 w-3 cursor-se-resize rounded-full"
                    style={{ background: "#ffffff", borderColor: color, borderWidth: 1, borderStyle: "solid" }}
                  />
                )}
              </div>
            );
          })}
          {draft &&
            (() => {
              const cls = classById.get(selectedClassId);
              const color = cls?.color ?? "#71717a";
              return (
                <div
                  className="pointer-events-none absolute rounded border-2"
                  style={{
                    left: `${draft.x * 100}%`,
                    top: `${draft.y * 100}%`,
                    width: `${draft.w * 100}%`,
                    height: `${draft.h * 100}%`,
                    borderColor: color,
                    background: color + "26",
                  }}
                />
              );
            })()}
        </div>
      </div>
    </div>
  );
}
