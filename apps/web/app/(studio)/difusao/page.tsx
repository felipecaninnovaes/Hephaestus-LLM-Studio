"use client";

import { useRouter } from "next/navigation";
import {
  IconImage,
  IconRefresh,
} from "@/components/icons";
import ForjaDifusaoSetup from "@/components/studio/ForjaDifusaoSetup";
import { Button } from "@/components/ui/Button";

export default function DifusaoPage() {
  const router = useRouter();

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
            <ForjaDifusaoSetup onJobCreated={handleJobCreated} />
          </div>
        </div>
      </div>
    </div>
  );
}
