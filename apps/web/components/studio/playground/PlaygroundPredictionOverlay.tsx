"use client";

import type { PredictionImage } from "@/types/studio";

export interface PlaygroundPredictionOverlayProps {
  prediction: PredictionImage;
  image?: { url: string; width: number; height: number };
  classes?: { name: string; color: string }[];
}

function classColor(
  cls: string,
  classes?: { name: string; color: string }[],
): string {
  return classes?.find((c) => c.name === cls)?.color ?? "#71717a";
}

export function PlaygroundPredictionOverlay({
  prediction: predImg,
  image: img,
  classes,
}: PlaygroundPredictionOverlayProps) {
  const hasBoxes = predImg.boxes.length > 0;

  return (
    <div className="glass-card overflow-hidden rounded-xl">
      {/* Imagem com overlay de boxes */}
      <div className="relative bg-zinc-900/50">
        {img ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={img.url}
            alt={predImg.filename}
            className="block w-full object-contain"
            style={{
              aspectRatio:
                img.width && img.height ? `${img.width}/${img.height}` : undefined,
            }}
            loading="lazy"
          />
        ) : (
          <div className="flex aspect-video flex-col items-center justify-center gap-1 text-zinc-600">
            <span className="font-mono text-2xs">{predImg.filename}</span>
            <span className="font-mono text-3xs text-zinc-500">
              imagem não encontrada no dataset
            </span>
          </div>
        )}

        {/* Bounding boxes overlay */}
        {hasBoxes &&
          predImg.boxes.map((box) => {
            const color = classColor(box.class, classes);
            return (
              <div
                key={`${box.class}-${box.x}-${box.y}-${box.w}-${box.h}-${box.conf}`}
                className="absolute"
                style={{
                  left: `${box.x * 100}%`,
                  top: `${box.y * 100}%`,
                  width: `${box.w * 100}%`,
                  height: `${box.h * 100}%`,
                  border: `1.5px solid ${color}`,
                  boxShadow: `0 0 0 1px ${color}33`,
                }}
              >
                {/* Badge de classe */}
                <span
                  className="absolute -top-2.5 left-0 flex items-center gap-1 rounded-sm px-1 py-px font-mono text-4xs font-medium leading-tight"
                  style={{
                    backgroundColor: `${color}22`,
                    color,
                    border: `1px solid ${color}44`,
                  }}
                >
                  {box.class}
                  <span style={{ opacity: 0.7 }}>{box.conf.toFixed(2)}</span>
                </span>
              </div>
            );
          })}
      </div>

      {/* Rodapé do card */}
      <div className="flex items-center justify-between px-3 py-2">
        <span
          className="min-w-0 truncate font-mono text-2xs text-zinc-400"
          title={predImg.filename}
        >
          {predImg.filename}
        </span>
        <span
          className={`shrink-0 rounded-full px-1.5 py-0.5 font-mono text-3xs ${
            hasBoxes
              ? "bg-brand-500/15 text-brand-300"
              : "bg-white/5 text-zinc-500"
          }`}
        >
          {predImg.boxes.length === 0
            ? "sem detecção"
            : `${predImg.boxes.length} box${predImg.boxes.length !== 1 ? "es" : ""}`}
        </span>
      </div>
    </div>
  );
}
