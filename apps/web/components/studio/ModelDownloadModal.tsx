"use client";

import { useCallback, useState } from "react";
import { Button, Modal, SegmentedControl, showToast } from "@/components/ui";
import { IconDownload } from "@/components/icons";
import { downloadModel } from "@/lib/models";
import { modelErrorMessage, type Model } from "@/types/studio";

interface ModelDownloadModalProps {
  open: boolean;
  onClose: () => void;
  onDownloaded: (model: Model) => void;
}

const ENGINE_OPTIONS = [
  { id: "yolo", label: "YOLO (Detecção / Treino)" },
  { id: "world", label: "YOLO-World (AutoTracker)" },
  { id: "diffusion", label: "Difusão (Geração)" },
  { id: "clip", label: "CLIP (Embeddings)" },
];

export default function ModelDownloadModal({
  open,
  onClose,
  onDownloaded,
}: ModelDownloadModalProps) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [engine, setEngine] = useState<"yolo" | "world" | "diffusion" | "clip">("yolo");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reset = useCallback(() => {
    setUrl("");
    setName("");
    setEngine("yolo");
    setError(null);
  }, []);

  const handleClose = useCallback(() => {
    if (busy) return;
    reset();
    onClose();
  }, [busy, reset, onClose]);

  async function handleSubmit() {
    const trimmedUrl = url.trim();
    if (!trimmedUrl) return;
    setBusy(true);
    setError(null);
    try {
      const model = await downloadModel({
        url: trimmedUrl,
        engine,
        name: name.trim() || undefined,
      });
      showToast("Modelo baixado com sucesso.", "success");
      onDownloaded(model);
      reset();
      onClose();
    } catch (err: unknown) {
      const code =
        typeof err === "object" && err !== null && "code" in err
          ? (err as { code: string }).code
          : "";
      setError(modelErrorMessage(code));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={handleClose}
      title="Baixar modelo por URL"
      description="Download server-side de pesos (.pt ou .safetensors)"
      icon={<IconDownload className="h-4 w-4" />}
      maxWidth="md"
      busy={busy}
    >
      <div className="space-y-4">
        {/* Engine selector */}
        <div>
          <span className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400">
            Engine / Tipo
          </span>
          <SegmentedControl
            options={ENGINE_OPTIONS}
            value={engine}
            onChange={(v) => setEngine(v as "yolo" | "world" | "diffusion" | "clip")}
            ariaLabel="Tipo de modelo"
            className="w-full justify-start"
          />
        </div>

        {/* URL */}
        <div>
          <label htmlFor="model-download-url" className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400">
            URL do modelo
          </label>
          <input
            id="model-download-url"
            type="url"
            value={url}
            onChange={(e) => {
              setUrl(e.target.value);
              setError(null);
            }}
            placeholder={
              engine === "world"
                ? "https://github.com/…/yolov8x-worldv2.pt"
                : engine === "diffusion"
                ? "https://huggingface.co/…/diffusion.safetensors"
                : engine === "clip"
                ? "https://huggingface.co/…/clip-vit-base.safetensors"
                : "https://huggingface.co/…/best.pt"
            }
            disabled={busy}
            className="w-full rounded-lg border border-zinc-700/60 bg-black/40 px-3 py-1.5 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-55"
          />
        </div>

        {/* Nome opcional */}
        <div>
          <label htmlFor="model-download-name" className="mb-1.5 block font-mono text-2xs font-medium uppercase tracking-[0.08em] text-zinc-400">
            Nome (opcional)
          </label>
          <input
            id="model-download-name"
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={engine === "world" ? "ex: yolov8x-worldv2.pt" : "ex: yolo11m_custom.pt"}
            disabled={busy}
            maxLength={255}
            className="w-full rounded-lg border border-zinc-700/60 bg-black/40 px-3 py-1.5 text-xs font-mono text-zinc-100 placeholder:text-zinc-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-55"
          />
        </div>

        {/* Aviso sobre 403 */}
        <p className="text-2xs leading-relaxed text-zinc-500">
          O download é processado server-side. Se a feature estiver desabilitada
          no ambiente (variável{" "}
          <code className="font-mono text-zinc-400">
            MODEL_DOWNLOAD_ALLOWED_HOSTS
          </code>
          ), a operação retornará erro.
        </p>

        {/* Erro */}
        {error && (
          <div className="rounded-lg border border-status-danger/30 bg-status-danger/[0.08] px-3 py-2 text-xs text-status-danger">
            {error}
          </div>
        )}

        {/* Ações */}
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
            disabled={!url.trim() || busy}
            loading={busy}
          >
            {busy ? "Baixando…" : "Baixar"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
