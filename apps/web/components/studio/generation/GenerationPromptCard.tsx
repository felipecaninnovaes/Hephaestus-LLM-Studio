"use client";

import { IconX } from "@/components/icons";
import { Button } from "@/components/ui/Button";

export interface GenerationPromptCardProps {
  prompt: string;
  setPrompt: (val: string) => void;
  negativePrompt: string;
  setNegativePrompt: (val: string) => void;
  showNegative: boolean;
  setShowNegative: (val: boolean) => void;
  disabled: boolean;
}

export function GenerationPromptCard({
  prompt,
  setPrompt,
  negativePrompt,
  setNegativePrompt,
  showNegative,
  setShowNegative,
  disabled,
}: GenerationPromptCardProps) {
  return (
    <div className="space-y-3">
      {/* ══ Prompt ══ */}
      <div className="space-y-1.5">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
          <label
            htmlFor="gen-prompt"
            className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-300"
          >
            Prompt
          </label>
          <span className="shrink-0 font-mono text-3xs text-zinc-500">
            {prompt.length}/4000
          </span>
        </div>
        <textarea
          id="gen-prompt"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value.slice(0, 4000))}
          disabled={disabled}
          placeholder="Descrição da imagem desejada…"
          rows={3}
          className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition resize-none"
        />
      </div>

      {/* ══ Negative Prompt (colapsável) ══ */}
      {!showNegative ? (
        <button
          type="button"
          onClick={() => setShowNegative(true)}
          className="font-mono text-2xs text-zinc-400 hover:text-zinc-200 transition-colors cursor-pointer"
        >
          + Prompt Negativo
        </button>
      ) : (
        <div className="space-y-1.5">
          <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
            <label
              htmlFor="gen-negative-prompt"
              className="font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-300"
            >
              Prompt Negativo
            </label>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setShowNegative(false);
                setNegativePrompt("");
              }}
              className="h-5 px-1 text-3xs text-zinc-400 hover:text-zinc-200"
            >
              <IconX className="size-3" />
              Remover
            </Button>
          </div>
          <textarea
            id="gen-negative-prompt"
            value={negativePrompt}
            onChange={(e) => setNegativePrompt(e.target.value.slice(0, 2000))}
            disabled={disabled}
            placeholder="Elementos que você NÃO quer na imagem…"
            rows={2}
            className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 text-xs text-zinc-100 placeholder:text-zinc-500 focus:outline-none focus:border-brand-500 focus-visible:ring-1 focus-visible:ring-brand-500/50 transition resize-none"
          />
        </div>
      )}
    </div>
  );
}
