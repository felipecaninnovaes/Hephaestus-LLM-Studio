"use client";

import { Suspense, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
  IconImage,
  IconRefresh,
} from "@/components/icons";
import ForjaDifusaoSetup from "@/components/studio/ForjaDifusaoSetup";
import { Button } from "@/components/ui/Button";
import type { DiffusionPreset } from "@/types/studio";

function DifusaoContent() {
  const router = useRouter();
  const searchParams = useSearchParams();

  const [resumeCheckpoint, setResumeCheckpoint] = useState<{
    id: string;
    name: string;
    epoch?: number;
  } | null>(null);
  const [epochOffset, setEpochOffset] = useState<number>(0);
  const [initialPreset, setInitialPreset] = useState<Partial<DiffusionPreset> | undefined>(undefined);
  const [initialDatasetId, setInitialDatasetId] = useState<string>("");

  useEffect(() => {
    // 1. Tenta carregar dados de retomada passados via sessionStorage
    try {
      const stored = sessionStorage.getItem("hephaestus_diffusion_resume");
      if (stored) {
        sessionStorage.removeItem("hephaestus_diffusion_resume");
        const parsed = JSON.parse(stored);
        if (parsed.resumeCheckpoint) {
          setResumeCheckpoint(parsed.resumeCheckpoint);
        }
        if (typeof parsed.epochOffset === "number") {
          setEpochOffset(parsed.epochOffset);
        }
        if (parsed.initialPreset) {
          setInitialPreset(parsed.initialPreset);
        }
        if (parsed.datasetId) {
          setInitialDatasetId(parsed.datasetId);
        }
        return;
      }
    } catch {
      // Ignora falha de parse/storage
    }

    // 2. Fallback para query params da URL
    const cpId = searchParams.get("checkpointId");
    const cpName = searchParams.get("checkpointName");
    const offsetStr = searchParams.get("epochOffset") || searchParams.get("epoch");
    const dsId = searchParams.get("datasetId");
    if (dsId) {
      setInitialDatasetId(dsId);
    }
    if (cpId) {
      setResumeCheckpoint({
        id: cpId,
        name: cpName || "Checkpoint",
        epoch: offsetStr ? parseInt(offsetStr, 10) : undefined,
      });
      if (offsetStr) {
        setEpochOffset(parseInt(offsetStr, 10) || 0);
      }
    }
  }, [searchParams]);

  function handleJobCreated(jobId: string) {
    // Navega para Execuções com query param para auto-seleção do job criado.
    router.push(`/jobs?job=${jobId}`);
  }

  return (
    <div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
      {/* ═══════════════════════════════════════════════
          HEADER — FORJA DIFUSÃO LORA
          ═══════════════════════════════════════════════ */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-sky-500/30 bg-sky-500/15 text-sky-400 backdrop-blur-sm">
              <IconImage className="size-4" />
            </span>
            <h1 className="font-display text-lg font-bold text-white tracking-tight">
              Forja Difusão LoRA
            </h1>
            <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 font-mono text-[11px] uppercase tracking-caps text-sky-400 backdrop-blur-sm">
              Diffusers Engine
            </span>
          </div>
          <p className="mt-1 text-xs text-zinc-400 max-w-2xl">
            Forje adaptadores LoRA para modelos de difusão (SDXL, Flux, SD 1.5) a partir das imagens e legendas dos seus datasets.
          </p>
        </div>

        <div className="flex items-center space-x-2.5 shrink-0">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => router.push("/jobs")}
            title="Ver execuções em andamento"
          >
            <IconRefresh className="size-3.5" />
            <span>Ver Execuções</span>
          </Button>
        </div>
      </div>

      {/* ═══════════════════════════════════════════════
          FORMULÁRIO DE SETUP — COLUNA CENTRALIZADA
          ═══════════════════════════════════════════════ */}
      <div className="flex justify-center">
        <div className="w-full max-w-2xl">
          <div className="glass-card rounded-2xl p-6 border border-white/10 shadow-2xl backdrop-blur-md">
            <ForjaDifusaoSetup
              onJobCreated={handleJobCreated}
              initialPreset={initialPreset}
              initialDatasetId={initialDatasetId}
              resumeCheckpoint={resumeCheckpoint}
              epochOffset={epochOffset}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

export default function DifusaoPage() {
  return (
    <Suspense fallback={null}>
      <DifusaoContent />
    </Suspense>
  );
}
