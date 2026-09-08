"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconTarget, IconX } from "@/components/icons";
import { ApiError } from "@/lib/api";
import { startAutotrackerJob } from "@/lib/autotracker";
import { autotrackerErrorMessage } from "@/types/studio";
import { showToast } from "./Toast";

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
  const sliderRef = useRef<HTMLInputElement>(null);
  const [conf, setConf] = useState<number>(CONF_DEFAULT);
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setConf(CONF_DEFAULT);
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => sliderRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open || busy) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

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
      router.push("/jobs");
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
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={() => {
        if (!busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="autotracker-title"
        className="glass-modal relative w-full max-w-md rounded-2xl p-6 text-zinc-100 shadow-2xl max-h-[90vh] overflow-y-auto"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-white/10 pb-4">
          <div className="flex items-center space-x-2.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400">
              <IconTarget className="h-4 w-4" />
            </div>
            <div>
              <h3
                id="autotracker-title"
                className="font-display text-sm font-bold text-white"
              >
                AutoTracker
              </h3>
              <p className="truncate font-mono text-[10px] text-zinc-400" title={datasetTitle}>
                {datasetTitle}
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            aria-label="Fechar modal"
            className="rounded-lg border border-transparent bg-transparent p-1 text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
          >
            <IconX className="h-4 w-4" />
          </button>
        </div>

        <form onSubmit={handleSubmit} className="mt-4 space-y-4 text-xs">
          {topError && (
            <p
              role="alert"
              className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
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
              className="w-full rounded-xl border border-zinc-800 bg-black/40 px-3 py-2 font-mono text-zinc-400 focus:border-brand-500 focus:outline-none opacity-70"
            >
              <option value="mock">mock (determinístico)</option>
            </select>
          </div>

          {/* Confiança (slider) */}
          <div>
            <label
              htmlFor="at-conf"
              className="tracking-caps mb-1 flex items-center justify-between font-mono text-[11px] font-medium uppercase text-zinc-300"
            >
              <span>Confiança mínima</span>
              <span className="font-mono text-brand-300 normal-case">
                {conf.toFixed(2)}
              </span>
            </label>
            <input
              id="at-conf"
              ref={sliderRef}
              type="range"
              min={CONF_MIN}
              max={CONF_MAX}
              step={CONF_STEP}
              value={conf}
              onChange={(e) => setConf(parseFloat(e.target.value))}
              disabled={busy}
              className="w-full accent-brand-500"
            />
            <div className="mt-0.5 flex justify-between font-mono text-[10px] text-zinc-500">
              <span>{CONF_MIN}</span>
              <span>{CONF_MAX}</span>
            </div>
          </div>

          {/* CTA */}
          <div className="flex justify-end space-x-2 pt-2">
            <button
              type="button"
              onClick={onClose}
              disabled={busy}
              className="inline-flex h-9 items-center justify-center gap-2 rounded-lg border border-transparent bg-transparent px-4 text-xs font-medium whitespace-nowrap text-zinc-300 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              Cancelar
            </button>
            <button
              type="submit"
              disabled={busy}
              className="inline-flex h-10 items-center justify-center gap-2 rounded-lg border border-brand-500/30 bg-brand-500/[0.12] px-5 text-xs font-semibold whitespace-nowrap text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] transition hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
            >
              {busy ? "Iniciando…" : "Executar"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
