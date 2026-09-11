"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconTarget } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { Select, type SelectOption } from "@/components/ui/Select";
import { ApiError } from "@/lib/api";
import { startAutotrackerJob } from "@/lib/autotracker";
import { listModels } from "@/lib/models";
import {
  autotrackerErrorMessage,
  modelSourceLabel,
  type Model,
} from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";

const CONF_MIN = 0.3;
const CONF_MAX = 0.95;
const CONF_DEFAULT = 0.65;
const CONF_STEP = 0.01;

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
}

export default function AutoTrackerModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
}: Props) {
  const router = useRouter();
  const [conf, setConf] = useState<number>(CONF_DEFAULT);
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);
  const sliderRef = useRef<HTMLInputElement>(null);

  /* ── World models (best-effort) ── */
  const [worldModels, setWorldModels] = useState<Model[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [selectedModelId, setSelectedModelId] = useState<string>("");

  useEffect(() => {
    if (!open) return;
    let active = true;
    setModelsLoading(true);
    listModels()
      .then((res) => {
        if (active) {
          setWorldModels(res.items.filter((m) => m.engine === "world"));
          setModelsLoading(false);
        }
      })
      .catch(() => {
        if (active) {
          setWorldModels([]);
          setModelsLoading(false);
        }
      });
    return () => {
      active = false;
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    setConf(CONF_DEFAULT);
    setSelectedModelId("");
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => sliderRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);

  const modelOptions = useMemo<SelectOption<string>[]>(() => {
    const mockOpt: SelectOption<string> = {
      value: "",
      label: "Mock (determinístico)",
      description: "Gera bounding boxes determinísticas — sem modelo real.",
    };
    const worldOpts: SelectOption<string>[] = worldModels.map((m) => ({
      value: m.id,
      label: m.name,
      badge:
        m.source === "train" ? (
          <span className="rounded-full border border-brand-500/35 bg-brand-500/10 px-1.5 py-0.5 font-mono text-[10px] text-brand-400">
            Treino
          </span>
        ) : (
          <span className="rounded-full border border-white/15 bg-white/[0.06] px-1.5 py-0.5 font-mono text-[10px] text-zinc-300">
            {modelSourceLabel(m.source)}
          </span>
        ),
    }));
    return [mockOpt, ...worldOpts];
  }, [worldModels]);

  if (!open) return null;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

    if (conf < CONF_MIN || conf > CONF_MAX) {
      setTopError(`Confiança deve estar entre ${CONF_MIN} e ${CONF_MAX}.`);
      return;
    }

    setBusy(true);
    try {
      const result = await startAutotrackerJob({
        datasetId,
        model: "mock",
        conf,
        ...(selectedModelId ? { modelId: selectedModelId } : {}),
      });
      showToast(
        `AutoTracker iniciado (posição ${result.queuePosition ?? "—"} na fila).`,
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
        if (err.code === "not_found") {
          if (selectedModelId) {
            setTopError("Modelo não encontrado — atualize a lista.");
          } else {
            setTopError("Recurso não encontrado.");
          }
        } else {
          setTopError(autotrackerErrorMessage(err.code));
        }
        return;
      }
      setTopError("Falha ao criar job de AutoTracker.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="AutoTracker"
      description={
        <span
          className="block truncate font-mono text-[10px] text-zinc-400"
          title={datasetTitle}
        >
          {datasetTitle}
        </span>
      }
      icon={<IconTarget className="h-4 w-4" />}
      maxWidth="md"
      busy={busy}
      ariaLabel="AutoTracker"
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
        <Select
          id="at-model"
          label="Modelo"
          options={modelOptions}
          value={selectedModelId}
          onChange={setSelectedModelId}
          placeholder="Selecione um modelo…"
          loading={modelsLoading}
          loadingText="Carregando modelos…"
          fontMono
          disabled={busy}
          size="default"
        />

        {worldModels.length === 0 && !modelsLoading && (
          <p className="font-mono text-[11px] text-zinc-500">
            Importe pesos world em{" "}
            <span className="text-zinc-400">Modelos &amp; Pesos</span> para usar
            um modelo real.
          </p>
        )}

        {/* Confiança (slider) */}
        <Slider
          ref={sliderRef}
          id="at-conf"
          label="Confiança mínima"
          min={CONF_MIN}
          max={CONF_MAX}
          step={CONF_STEP}
          value={conf}
          onChange={setConf}
          formatValue={(v) => v.toFixed(2)}
          disabled={busy}
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
            Executar
          </Button>
        </div>
      </form>
    </Modal>
  );
}
