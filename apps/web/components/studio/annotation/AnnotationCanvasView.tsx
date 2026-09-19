"use client";

import type { Dispatch, MouseEvent as ReactMouseEvent, RefObject, SetStateAction } from "react";
import { ZoomControl } from "@/components/ui/ZoomControl";
import type { ToolId } from "@/hooks/useAnnotationCanvas";
import type { BBoxData, ImageDetail } from "@/types/studio";

export interface AnnotationCanvasViewProps {
  detail: ImageDetail;
  frameRef: RefObject<HTMLDivElement | null>;
  activeTool: ToolId;
  zoom: number;
  setZoom: Dispatch<SetStateAction<number>>;
  pan: { x: number; y: number };
  frameWidth: number;
  frameHeight: number;
  boxes: BBoxData[];
  selectedBoxId: string | null;
  setSelectedBoxId: (id: string | null) => void;
  draft: BBoxData | null;
  selectedClassId: string;
  classById: Map<string, { name: string; color: string }>;
  onFrameMouseDown: (e: ReactMouseEvent) => void;
  onBoxMouseDown: (e: ReactMouseEvent, box: BBoxData) => void;
}

export function AnnotationCanvasView({
  detail,
  frameRef,
  activeTool,
  zoom,
  setZoom,
  pan,
  frameWidth,
  frameHeight,
  boxes,
  selectedBoxId,
  setSelectedBoxId,
  draft,
  selectedClassId,
  classById,
  onFrameMouseDown,
  onBoxMouseDown,
}: AnnotationCanvasViewProps) {
  return (
    <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center overflow-auto bg-[#0b0f17] p-6">
      <ZoomControl
        value={zoom}
        onChange={setZoom}
        className="absolute top-4 left-6 z-20"
      />

      <div
        ref={frameRef}
        role="application"
        aria-label="Canvas de anotação. Ferramentas B, V, H. Escape limpa a seleção."
        className={`relative flex items-center justify-center overflow-hidden rounded-2xl border-2 border-zinc-700/80 border-t-white/20 bg-zinc-900/90 shadow-2xl backdrop-blur-sm transition-transform duration-200 focus-visible:outline-none focus-visible:border-brand-500/70 ${
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
          if (activeTool === "select") setSelectedBoxId(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape" && activeTool === "select") {
            setSelectedBoxId(null);
          }
        }}
      >
        <div className="absolute inset-0 bg-[radial-gradient(#ffffff_1px,transparent_1px)] opacity-20 [background-size:18px_18px]" />
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={detail.url}
          alt={detail.filename}
          className="absolute inset-0 h-full w-full pointer-events-none select-none"
          draggable={false}
        />

        {boxes.map((box, i) => {
          const cls = classById.get(box.classId);
          const color = cls?.color ?? "#71717a";
          const name = cls?.name ?? "classe";
          const isSelected = selectedBoxId === box.id;
          return (
            <button
              type="button"
              key={box.id}
              onMouseDown={(e) => onBoxMouseDown(e, box)}
              onClick={(e) => {
                e.stopPropagation();
                setSelectedBoxId(box.id);
              }}
              aria-label={`${name} ${i + 1} de ${boxes.length}${
                isSelected ? ", selecionada" : ""
              }`}
              aria-pressed={isSelected}
              className={`absolute cursor-move rounded border-2 p-0 transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/70 ${
                isSelected ? "shadow-lg ring-2 ring-white/50" : ""
              }`}
              style={{
                left: `${box.x * 100}%`,
                top: `${box.y * 100}%`,
                width: `${box.w * 100}%`,
                height: `${box.h * 100}%`,
                borderColor: color,
                background: `${color}26`,
              }}
            >
              <span
                className="pointer-events-none absolute -top-5 left-0 rounded px-1.5 py-0.5 font-mono text-3xs font-bold"
                style={{ background: color, color: "#09090b" }}
              >
                {name} #{i}
              </span>
              {isSelected && (
                <span
                  data-resize-handle="true"
                  aria-hidden="true"
                  className="absolute -right-1.5 -bottom-1.5 h-3 w-3 cursor-se-resize rounded-full"
                  style={{
                    background: "#ffffff",
                    borderColor: color,
                    borderWidth: 1,
                    borderStyle: "solid",
                  }}
                />
              )}
            </button>
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
                  background: `${color}26`,
                }}
              />
            );
          })()}
      </div>
    </div>
  );
}
