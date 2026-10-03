"use client";

import { useRouter } from "next/navigation";
import { useEffect, useMemo, useRef, useState } from "react";
import { IconPlay } from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import type { SelectRefHandle } from "@/components/ui/Select";
import { showToast } from "@/components/ui/Toast";
import { ApiError } from "@/lib/api";
import { openActionCenter } from "@/lib/events";
import { startYoloJob } from "@/lib/jobs";
import type { Orchestrator } from "@/lib/monitoring";
import { jobErrorMessage } from "@/types/studio";
import { estimateYoloVramGb } from "./ForjaYoloSetup";
import GpuDeviceSelect from "./GpuDeviceSelect";
import NodeSelect from "./NodeSelect";
import {
  EPOCHS_MAX,
	EPOCHS_MIN,
	YoloHyperparameters,
  type YoloHyperparametersValues,
} from "./YoloHyperparameters";

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
}

const DEFAULT_PARAMS: YoloHyperparametersValues = {
  model: "yolo11m",
  epochs: 100,
  batch: 16,
  imgsz: 640,
  lr0: "0.01",
  optimizer: "AdamW",
  augment: { mosaic: true, mixupFlip: true },
};

export default function TrainYoloModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
}: Props) {
  const router = useRouter();
  const firstRef = useRef<SelectRefHandle>(null);
	const [params, setParams] =
		useState<YoloHyperparametersValues>(DEFAULT_PARAMS);
	const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<
		string | null
	>(null);
	const [selectedGpuDevice, setSelectedGpuDevice] = useState<string | null>(
		null,
	);
	const [orchestratorsList, setOrchestratorsList] = useState<Orchestrator[]>(
		[],
	);
  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);
  useEffect(() => {
    if (!open) return;
    setParams(DEFAULT_PARAMS);
    setSelectedOrchestratorId(null);
		setSelectedGpuDevice(null);
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => firstRef.current?.focus(), 30);
    return () => clearTimeout(t);
  }, [open]);
  const parsedLr0 = parseFloat(params.lr0);
  const epochsValid =
    Number.isInteger(params.epochs) &&
    params.epochs >= EPOCHS_MIN &&
    params.epochs <= EPOCHS_MAX;
  const lr0Valid =
    !isNaN(parsedLr0) && parsedLr0 >= 1e-5 && parsedLr0 <= 0.1 + 1e-9;
	const estimatedVram = useMemo(
		() =>
			estimateYoloVramGb(
				params.model,
				params.batch,
				params.imgsz,
				params.optimizer,
			),
		[params.model, params.batch, params.imgsz, params.optimizer],
	);

	if (!open) return null;
  const handleParamChange = <K extends keyof YoloHyperparametersValues>(
    key: K,
    val: YoloHyperparametersValues[K],
  ) => {
    setParams((prev) => ({ ...prev, [key]: val }));
  };

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);

    if (!epochsValid) {
      setTopError(`Epochs deve ser entre ${EPOCHS_MIN} e ${EPOCHS_MAX}.`);
      return;
    }
    if (!lr0Valid) {
      setTopError("lr0 deve estar entre 0.00001 e 0.1.");
      return;
    }

    setBusy(true);
    try {
      const result = await startYoloJob({
        datasetId,
        model: params.model,
        epochs: params.epochs,
        batch: params.batch,
        imgsz: params.imgsz,
        lr0: parsedLr0,
        optimizer: params.optimizer,
        augment: params.augment,
        orchestratorId: selectedOrchestratorId || null,
        gpuDevice: (selectedOrchestratorId && selectedGpuDevice) ? selectedGpuDevice : undefined,
      });
      showToast(
        result.status === "preparing"
          ? "Treino aceito — preparando pacote (empacotando dataset…). Acompanhe no Centro de Ações."
          : `Job de treino criado (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
        {
          label: "Ver na Forja",
          onClick: () => router.push(`/jobs?job=${result.jobId}`),
        },
      );
      onJobCreated();
      onClose();
      openActionCenter();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
        setTopError(jobErrorMessage(err.code));
        return;
      }
      setTopError("Falha ao criar job de treino.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Treinar YOLO"
      description={
        <span
          className="truncate font-mono text-2xs text-zinc-400 block"
          title={datasetTitle}
        >
          {datasetTitle}
        </span>
      }
      icon={<IconPlay className="h-4 w-4" />}
      maxWidth="lg"
      busy={busy}
      ariaLabel="Treinar YOLO"
    >
      <form onSubmit={handleSubmit} className="space-y-4 text-xs">
        {topError && (
          <p
            role="alert"
            className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
          >
            {topError}
          </p>
        )}

        <YoloHyperparameters
          values={params}
          onChange={handleParamChange}
          disabled={busy}
          firstSelectRef={firstRef}
        />

        {/* Nó de Execução (ADR-0015 D2) */}
        <NodeSelect
          value={selectedOrchestratorId}
					onChange={(nodeId) => {
						setSelectedOrchestratorId(nodeId);
						setSelectedGpuDevice(null);
					}}
					onOrchestratorsLoaded={setOrchestratorsList}
					disabled={busy}
					size="default"
				/>

				{/* GPU de Execução (Fatia F2) */}
				<GpuDeviceSelect
					orchestratorId={selectedOrchestratorId}
					devices={
						orchestratorsList.find((o) => o.id === selectedOrchestratorId)
							?.gpuDevices ?? null
					}
					value={selectedGpuDevice}
					onChange={setSelectedGpuDevice}
					vramMinGb={estimatedVram}
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
          <Button
            type="submit"
            variant="primary"
            size="lg"
            loading={busy}
            leftIcon={<IconPlay className="size-3.5" />}
          >
            {busy ? "Iniciando…" : "Iniciar Treino"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
