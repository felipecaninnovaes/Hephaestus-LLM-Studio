"use client";

import { useMemo } from "react";
import { IconPlus, IconX } from "@/components/icons";
import { Button, Select, Slider, type SelectOption } from "@/components/ui";
import type { Model, LoraRef } from "@/types/studio";

export interface LoRAEditorProps {
  /** Lista de LoRAs selecionados (estado controlado). */
  value: LoraRef[];
  /** Callback ao mudar a lista completa. */
  onChange: (loras: LoraRef[]) => void;
  /** Todos os modelos de difusão (filtrados por engine=diffusion kind=lora na UI). */
  loraModels: Model[];
  /** Se o formulário está submetendo (desabilita controles). */
  disabled?: boolean;
  /** Máximo de LoRAs simultâneos. */
  maxItems?: number;
}

/**
 * Editor multi-LoRA (ADR-0023 D3).
 * Cada linha: Select de LoRA + Slider canônico de escala 0..2 (step 0.05).
 * Botão "+ Adicionar LoRA" até maxItems (default 4).
 * Opção "Nenhum (Modelo Base Puro)" sempre presente como primeira opção.
 */
export function LoRAEditor({
  value,
  onChange,
  loraModels,
  disabled = false,
  maxItems = 4,
}: LoRAEditorProps) {
  const loraOptions = useMemo<SelectOption<string>[]>(() => {
    const opts: SelectOption<string>[] = [
      {
        value: "",
        label: "Nenhum (Modelo Base Puro)",
        description: "Sem adaptador — pesos originais do base",
      },
    ];
    loraModels.forEach((m) => {
      const archLabel = m.arch ? ` · ${m.arch}` : "";
      opts.push({
        value: m.id,
        label: m.name,
        description: `LoRA${archLabel} · ${m.source}`,
      });
    });
    return opts;
  }, [loraModels]);

  const canAdd = value.length < maxItems && !disabled;

  function handleAdd() {
    if (!canAdd) return;
    // Add a new empty LoRA row (defaults to "Nenhum")
    onChange([...value, { modelId: "", scale: 1.0 }]);
  }

  function handleRemove(index: number) {
    const next = value.filter((_, i) => i !== index);
    onChange(next);
  }

  function handleModelChange(index: number, modelId: string) {
    const next = value.map((l, i) =>
      i === index ? { ...l, modelId } : l,
    );
    onChange(next);
  }

  function handleScaleChange(index: number, scale: number) {
    const next = value.map((l, i) =>
      i === index ? { ...l, scale } : l,
    );
    onChange(next);
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label className="font-mono text-[11px] font-semibold uppercase tracking-[0.08em] text-zinc-300">
          Adaptadores LoRA
        </label>
        {value.length > 0 && (
          <span className="font-mono text-[10px] text-brand-400">
            {value.length}/{maxItems}
          </span>
        )}
      </div>

      {/* Linhas de LoRA */}
      {value.map((lora, index) => (
        <div
          key={index}
          className="rounded-xl border border-white/8 bg-white/[0.02] p-2.5 space-y-2"
        >
          <div className="flex items-start gap-2">
            {/* Select do LoRA */}
            <div className="flex-1 min-w-0">
              <Select
                options={loraOptions}
                value={lora.modelId}
                onChange={(val) => handleModelChange(index, val)}
                disabled={disabled}
                placeholder="Selecione um LoRA"
                size="sm"
                fontMono
              />
            </div>

            {/* Botão remover */}
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={() => handleRemove(index)}
              disabled={disabled}
              title="Remover LoRA"
              className="mt-0.5 shrink-0 text-zinc-400 hover:text-rose-400"
            >
              <IconX className="size-3.5" />
            </Button>
          </div>

          {/* Slider de escala — Slider canônico */}
          {lora.modelId && (
            <Slider
              label="Escala"
              value={lora.scale}
              onChange={(v) => handleScaleChange(index, v)}
              min={0}
              max={2}
              step={0.05}
              disabled={disabled}
              formatValue={(v) => `${v.toFixed(2)}x`}
            />
          )}
        </div>
      ))}

      {/* Botão adicionar */}
      {canAdd && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={handleAdd}
          disabled={disabled}
          className="w-full border border-dashed border-white/10 text-zinc-400 hover:text-zinc-200 hover:border-white/20"
        >
          <IconPlus className="size-3.5" />
          Adicionar LoRA
        </Button>
      )}

      {/* Hint quando vazio */}
      {value.length === 0 && (
        <p className="font-mono text-[11px] text-zinc-500 text-center py-1">
          Nenhum LoRA — geração com pesos base puros.
        </p>
      )}
    </div>
  );
}

export default LoRAEditor;
