"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconTarget } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { ApiError } from "@/lib/api";
import { startAutotrackerJob } from "@/lib/autotracker";
import { autotrackerErrorMessage } from "@/types/studio";
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

  useEffect(() => {
    if (!open) return;
    setConf(CONF_DEFAULT);
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => sliderRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);

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
        setTopError(autotrackerErrorMessage(err.code));
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
        <span className="truncate font-mono text-[10px] text-zinc-400 block" title={datasetTitle}>
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
              className="rounded-lg border border-rose-500/30 bg-rose-500/10 backdrop-blur-sm px-3 py-2 text-xs text-rose-300"
            >
              {topError}
            </p>
          )}

          {/* Modelo */}
          <div>
            <label
              htmlFor="at-model"
              className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300"
            >
              Modelo
            </label>
            <select
              id="at-model"
              value="mock"
              disabled
              title="Modelo real chega em fatia futura."
              className="w-full rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm px-3 py-2 font-mono text-zinc-400 focus:border-brand-500 focus:outline-none opacity-70"
            >
              <option value="mock">mock (determinístico)</option>
            </select>
          </div>

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
            <Button
              type="submit"
              variant="primary"
              size="lg"
              loading={busy}
            >
              Executar
            </Button>
          </div>
        </form>
    </Modal>
  );
}
