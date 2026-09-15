"use client";

import { useCallback, useRef, useState } from "react";
import { Button, Modal, SegmentedControl, Select, showToast } from "@/components/ui";
import { IconUpload, IconBox } from "@/components/icons";
import { uploadModel } from "@/lib/models";
import { modelErrorMessage, type Model } from "@/types/studio";
import type { SelectOption } from "@/components/ui/Select";

interface ModelUploadModalProps {
  open: boolean;
  onClose: () => void;
  onUploaded: (model: Model) => void;
}

const ENGINE_OPTIONS = [
  { id: "yolo", label: "YOLO (Detecção / Treino)" },
  { id: "world", label: "YOLO-World (AutoTracker)" },
  { id: "diffusion", label: "Difusão (Geração)" },
  { id: "clip", label: "CLIP (Embeddings)" },
];

const KIND_OPTIONS: SelectOption<string>[] = [
  { value: "", label: "Detectar automaticamente (recomendado)" },
  { value: "lora", label: "LoRA (adaptador)" },
  { value: "checkpoint", label: "Checkpoint (modelo completo)" },
];

const ARCH_OPTIONS: SelectOption<string>[] = [
  { value: "", label: "Auto" },
  { value: "sdxl", label: "SDXL" },
  { value: "sd15", label: "SD 1.5" },
  { value: "flux-2-klein-4b", label: "FLUX.2 Klein 4B" },
];

export default function ModelUploadModal({
  open,
  onClose,
  onUploaded,
}: ModelUploadModalProps) {
  const [file, setFile] = useState<File | null>(null);
  const [name, setName] = useState("");
  const [engine, setEngine] = useState<"yolo" | "world" | "diffusion" | "clip">("yolo");
  const [kind, setKind] = useState("");
  const [arch, setArch] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const isDiffusionSafetensors =
    engine === "diffusion" &&
    file !== null &&
    file.name.toLowerCase().endsWith(".safetensors");

  const reset = useCallback(() => {
    setFile(null);
    setName("");
    setEngine("yolo");
    setKind("");
    setArch("");
    setError(null);
  }, []);

  const handleClose = useCallback(() => {
    if (busy) return;
    reset();
    onClose();
  }, [busy, reset, onClose]);

  function handleFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const f = e.target.files?.[0] ?? null;
    setFile(f);
    setError(null);
  }

  async function handleSubmit() {
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const model = await uploadModel({
        file,
        engine,
        name: name.trim() || undefined,
        kind: isDiffusionSafetensors && kind ? kind : undefined,
        arch: isDiffusionSafetensors && arch ? arch : undefined,
      });
      showToast("Modelo enviado com sucesso.", "success");
      onUploaded(model);
      reset();
      onClose();
    } catch (err: unknown) {
      const code =
        typeof err === "object" && err !== null && "code" in err
          ? (err as { code: string }).code
          : "";
      const message =
        typeof err === "object" && err !== null && "message" in err
          ? (err as { message: string }).message
          : "";
      setError(modelErrorMessage(code, message));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={handleClose}
      title="Enviar pesos"
      description="Upload de arquivo .pt ou .safetensors"
      icon={<IconUpload className="h-4 w-4" />}
      maxWidth="md"
      busy={busy}
    >
      <div className="space-y-4">
        {/* File picker */}
        <div>
          <label
            htmlFor="model-upload-file"
            className="mb-1.5 block font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-400"
          >
            Arquivo
          </label>
          <input
            ref={inputRef}
            id="model-upload-file"
            type="file"
            accept=".pt,.safetensors"
            onChange={handleFileChange}
            disabled={busy}
            className="sr-only"
          />
          <button
            type="button"
            onClick={() => inputRef.current?.click()}
            disabled={busy}
            aria-label="Selecionar arquivo .pt ou .safetensors"
            className={`flex w-full items-center gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors cursor-pointer ${
              file
                ? "border-brand-500/30 bg-brand-500/10"
                : "border-zinc-700/60 bg-black/40 hover:border-zinc-600 hover:bg-black/50"
            } disabled:opacity-55`}
          >
            <IconBox className="size-4 shrink-0 text-zinc-400" />
            <div className="min-w-0 flex-1">
              {file ? (
                <>
                  <div className="truncate text-xs font-medium text-zinc-100">
                    {file.name}
                  </div>
                  <div className="font-mono text-[11px] text-zinc-400">
                    {(file.size / (1024 * 1024)).toFixed(1)} MB
                  </div>
                </>
              ) : (
                <div className="text-xs text-zinc-400">
                  Selecionar arquivo .pt ou .safetensors…
                </div>
              )}
            </div>
          </button>
        </div>

        {/* Nome opcional */}
        <div>
          <label className="mb-1.5 block font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-400">
            Nome (opcional)
          </label>
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="ex: model.safetensors ou best_v2.pt"
            disabled={busy}
            maxLength={255}
            className="w-full rounded-lg border border-zinc-700/60 bg-black/40 px-3 py-1.5 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-55"
          />
        </div>

        {/* Engine selector */}
        <div>
          <label className="mb-1.5 block font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-400">
            Engine / Tipo
          </label>
          <SegmentedControl
            options={ENGINE_OPTIONS}
            value={engine}
            onChange={(v) => {
              setEngine(v as "yolo" | "world" | "diffusion" | "clip");
              // Limpar classificação ao mudar de engine
              setKind("");
              setArch("");
            }}
            ariaLabel="Tipo de modelo"
            className="w-full justify-start"
          />
        </div>

        {/* Classificação opcional — apenas para difusão + safetensors */}
        {isDiffusionSafetensors && (
          <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3.5 space-y-3">
            <div className="flex items-center gap-2">
              <span className="font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-400">
                Classificação (opcional)
              </span>
            </div>

            <Select
              id="model-upload-kind"
              label="Kind"
              options={KIND_OPTIONS}
              value={kind}
              onChange={(val) => {
                setKind(val);
                // Limpar arch se não faz sentido
                if (val !== "checkpoint" && arch) {
                  // Permitir arch para lora também (confirmação), manter valor
                }
              }}
              placeholder="Selecione o kind…"
              disabled={busy}
              fontMono
              size="default"
            />

            <Select
              id="model-upload-arch"
              label="Arquitetura"
              options={ARCH_OPTIONS}
              value={arch}
              onChange={(val) => setArch(val)}
              placeholder="Selecione a arquitetura…"
              disabled={busy}
              fontMono
              size="default"
            />

            <p className="font-mono text-[11px] text-zinc-500 leading-normal">
              O estúdio detecta a arquitetura pelo arquivo. Use os seletores só se o upload falhar com erro de classificação.
            </p>
          </div>
        )}

        {/* Erro */}
        {error && (
          <div className="rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.08] px-3 py-2 text-xs text-[#ef4444]">
            {error}
          </div>
        )}

        {/* Ações — One CTA */}
        <div className="flex items-center justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={handleSubmit}
            disabled={!file || busy}
            loading={busy}
          >
            Enviar
          </Button>
        </div>
      </div>
    </Modal>
  );
}
