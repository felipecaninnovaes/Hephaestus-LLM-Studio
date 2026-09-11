"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconSparkles } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { ApiError } from "@/lib/api";
import { startAutolabelJob } from "@/lib/autolabel";
import { autolabelErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import NodeSelect from "./NodeSelect";

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
}

export default function AutoLabelModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
}: Props) {
  const router = useRouter();
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setPrompt("");
    setSelectedOrchestratorId(null);
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => inputRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);

  if (!open) return null;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);
    setBusy(true);

    try {
      const result = await startAutolabelJob({
        datasetId,
        model: "mock",
        ...(prompt.trim() ? { prompt: prompt.trim() } : {}),
        ...(selectedOrchestratorId ? { orchestratorId: selectedOrchestratorId } : {}),
      });
      showToast(
        `AutoLabel iniciado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );
      onClose();
      onJobCreated();
      openActionCenter();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
        setTopError(autolabelErrorMessage(err.code));
        return;
      }
      setTopError("Falha ao criar job de AutoLabel.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="AutoLabel"
      description={
        <span
          className="block truncate font-mono text-[10px] text-zinc-400"
          title={datasetTitle}
        >
          {datasetTitle}
        </span>
      }
      icon={<IconSparkles className="h-4 w-4" />}
      maxWidth="md"
      busy={busy}
      ariaLabel="AutoLabel"
    >
      <form onSubmit={handleSubmit} className="space-y-4 text-xs">
        {topError && (
          <p
            role="alert"
            className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300 backdrop-blur-sm"
          >
            {topError}
          </p>
        )}

        {/* Modelo */}
        <div>
          <label className="tracking-caps mb-1.5 block font-mono text-[11px] font-medium uppercase text-zinc-300">
            Modelo
          </label>
          <div className="flex items-center justify-between rounded-xl border border-zinc-800 bg-black/40 px-3 py-2.5 backdrop-blur-sm">
            <div className="space-y-0.5">
              <span className="font-mono text-xs text-zinc-200">Mock (determinístico)</span>
              <p className="font-mono text-[11px] text-zinc-500">
                Gera legendas determinísticas locais sem GPU externa.
              </p>
            </div>
            <span className="rounded-full border border-brand-500/35 bg-brand-500/10 px-2 py-0.5 font-mono text-[10px] text-brand-400">
              v1 local
            </span>
          </div>
        </div>

        {/* Prompt livre / instrução */}
        <Input
          ref={inputRef}
          id="al-prompt"
          label="Prompt / Instrução (opcional)"
          placeholder="Ex.: Descreva os objetos em primeiro plano e iluminação…"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          disabled={busy}
          hint="Instrução adicional repassada ao gerador de legendas."
        />

        {/* Nó de Execução (ADR-0015 D2) */}
        <NodeSelect
          value={selectedOrchestratorId}
          onChange={setSelectedOrchestratorId}
          disabled={busy}
          size="default"
        />

        {/* CTA */}
        <div className="flex justify-end space-x-2 pt-2">
          <Button
            type="button"
            variant="ghost"
            size="md"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button type="submit" variant="primary" size="lg" loading={busy}>
            Executar AutoLabel
          </Button>
        </div>
      </form>
    </Modal>
  );
}
