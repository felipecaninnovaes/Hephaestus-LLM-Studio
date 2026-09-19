"use client";

import { useRouter } from "next/navigation";
import {
  IconBoxSelect,
  IconLayers,
  IconTarget,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Kbd } from "@/components/ui/Kbd";
import type { ToolId } from "@/hooks/useAnnotationCanvas";
import type { BBoxData, Dataset, StudioClass } from "@/types/studio";

export interface AnnotationSidebarProps {
  dataset: Dataset;
  classes: StudioClass[];
  activeTool: ToolId;
  setActiveTool: (tool: ToolId) => void;
  selectedClassId: string;
  setSelectedClassId: (id: string) => void;
  selectedBox: BBoxData | null;
  saving: boolean;
  dirty: boolean;
  onSave: () => void;
  onOpenClasses: () => void;
}

const TOOLS: { id: ToolId; label: string; key: string }[] = [
  { id: "bbox", label: "Caixa Delimitadora (BBox)", key: "B" },
  { id: "select", label: "Mover / Selecionar", key: "V" },
  { id: "pan", label: "Arrastar Canvas", key: "H" },
];

function toolIcon(id: ToolId) {
  if (id === "bbox") return IconBoxSelect;
  if (id === "select") return IconTarget;
  return IconLayers;
}

export function AnnotationSidebar({
  dataset,
  classes,
  activeTool,
  setActiveTool,
  selectedClassId,
  setSelectedClassId,
  selectedBox,
  saving,
  dirty,
  onSave,
  onOpenClasses,
}: AnnotationSidebarProps) {
  const router = useRouter();
  const hasClasses = classes.length > 0;

  return (
    <div className="w-full space-y-5 overflow-visible border-r border-zinc-800/80 bg-zinc-950/60 p-4 backdrop-blur-sm md:w-80 md:overflow-y-auto">
      <Button
        type="button"
        variant="secondary"
        size="md"
        onClick={() => router.push(`/datasets/${dataset.id}`)}
        className="w-full justify-start cursor-pointer"
      >
        <svg
          aria-hidden="true"
          focusable="false"
          className="h-3.5 w-3.5 shrink-0"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.7"
          viewBox="0 0 24 24"
        >
          <polyline points="15 18 9 12 15 6" />
        </svg>
        <span
          title={`Voltar para a galeria · ${dataset.title}`}
          className="min-w-0 flex-1 truncate text-left"
        >
          Voltar para a galeria · {dataset.title}
        </span>
      </Button>

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
                className={`inline-flex h-9 w-full items-center gap-2 rounded-lg border px-3 text-left text-xs font-medium whitespace-nowrap transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55 cursor-pointer ${
                  isActive
                    ? "border-brand-500/30 bg-brand-500/[0.18] text-brand-300"
                    : "border-transparent bg-transparent text-zinc-300 hover:bg-white/[0.06] hover:text-white"
                }`}
              >
                <ToolIcon />
                <span className="flex items-center gap-1.5 flex-1">
                  <span>{tool.label}</span>
                  <Kbd className="ml-auto">{tool.key}</Kbd>
                </span>
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
                className={`flex w-full items-center justify-between rounded-xl p-2 transition-all cursor-pointer ${
                  selectedClassId === cls.id
                    ? "border border-zinc-700 bg-zinc-800 text-white"
                    : "text-zinc-400 hover:bg-zinc-900/60"
                }`}
              >
                <div className="flex items-center space-x-2">
                  <span
                    className="h-2.5 w-2.5 rounded-full"
                    style={{ background: cls.color }}
                  />
                  <span>
                    {cls.idx}: {cls.name}
                  </span>
                </div>
                <Kbd size="sm">{cls.idx + 1}</Kbd>
              </button>
            ))}
          </div>
        ) : (
          <p className="text-xs text-zinc-400">
            Este dataset ainda não tem classes — use &quot;Gerenciar
            classes&quot; para criar.
          </p>
        )}
        <Button
          type="button"
          variant="secondary"
          size="md"
          onClick={onOpenClasses}
          className="mt-2 w-full"
        >
          Gerenciar classes
        </Button>
      </div>

      <div className="border-t border-zinc-800 pt-4 font-mono text-xs text-zinc-400">
        <span className="tracking-caps mb-2 block text-3xs text-zinc-400 uppercase">
          Coordenadas YOLO (Norm.)
        </span>
        <div className="space-y-1 rounded-xl border border-zinc-800 bg-zinc-900 p-2.5 text-2xs">
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
            W:{" "}
            <span className="text-zinc-200">
              {selectedBox ? selectedBox.w.toFixed(6) : "—"}
            </span>
          </div>
          <div>
            H:{" "}
            <span className="text-zinc-200">
              {selectedBox ? selectedBox.h.toFixed(6) : "—"}
            </span>
          </div>
          {selectedBox?.conf !== null && selectedBox?.conf !== undefined && (
            <div>
              Confiança:{" "}
              <span className="text-zinc-200">
                {selectedBox.conf.toFixed(3)}
              </span>
            </div>
          )}
          {selectedBox?.origin && (
            <div>
              Origem:{" "}
              <span className="text-zinc-200">{selectedBox.origin}</span>
            </div>
          )}
          {selectedBox?.trackId !== null &&
            selectedBox?.trackId !== undefined && (
              <div>
                Track ID:{" "}
                <span className="text-zinc-200">{selectedBox.trackId}</span>
              </div>
            )}
        </div>

        <Button
          type="button"
          variant="primary"
          size="md"
          onClick={onSave}
          disabled={saving || !hasClasses}
          loading={saving}
          className="mt-4 w-full"
        >
          {saving ? "Salvando…" : `${dirty ? "● " : ""}Salvar Anotações`}
        </Button>
      </div>
    </div>
  );
}
