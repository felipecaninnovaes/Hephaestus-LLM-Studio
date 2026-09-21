"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconPlay,
  IconRefresh,
} from "@/components/icons";
import ForjaYoloSetup from "@/components/studio/ForjaYoloSetup";
import { Button } from "@/components/ui/Button";
import { parseYoloRerun, type YoloFormPreset } from "@/lib/paramsToPreset";

export default function TreinoPage() {
  const router = useRouter();
  // Rerun YOLO (C1): ActionCenter escreve `heph_rerun_yolo` — lido uma vez,
  // removido, e usado para pré-preencher dataset+params do form.
  const [rerunDatasetId, setRerunDatasetId] = useState<string>("");
  const [rerunParams, setRerunParams] = useState<YoloFormPreset | undefined>(
    undefined,
  );
  const [rerunWeightsId, setRerunWeightsId] = useState<string>("");
  const [rerunOutputName, setRerunOutputName] = useState<string>("");
  const [rerunReady, setRerunReady] = useState(false);

  useEffect(() => {
    try {
      const stored = sessionStorage.getItem("heph_rerun_yolo");
      if (stored) {
        sessionStorage.removeItem("heph_rerun_yolo");
        const parsed = parseYoloRerun(stored);
        if (parsed) {
          setRerunDatasetId(parsed.datasetId);
          setRerunParams(parsed.params);
          setRerunWeightsId(parsed.weightsId);
          setRerunOutputName(parsed.outputName);
        }
      }
    } catch {
      // Ignora falha de parse/storage
    } finally {
      setRerunReady(true);
    }
  }, []);

  function handleJobCreated(jobId: string) {
    // Navega para Execuções com query param para auto-seleção do job criado.
    router.push(`/jobs?job=${jobId}`);
  }

  return (
    <div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
      {/* ═══════════════════════════════════════════════
          HEADER — TREINO YOLO
          ═══════════════════════════════════════════════ */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/10 pb-4">
        <div>
          <div className="flex items-center space-x-2.5">
            <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
              <IconPlay className="size-4" />
            </span>
            <h1 className="font-display text-lg font-bold text-white tracking-tight">
              Treino YOLO
            </h1>
            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-2xs uppercase tracking-caps text-zinc-400 backdrop-blur-sm">
              Ultralytics Engine
            </span>
          </div>
          <p className="mt-1 text-xs text-zinc-400 max-w-2xl">
            Configure hiperparâmetros e inicie um novo treinamento de visão computacional.
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
          FORMULÁRIO DE SETUP — COLUNA ÚNICA CENTRALIZADA
          ═══════════════════════════════════════════════ */}
      <div className="flex justify-center">
        <div className="w-full max-w-xl">
          <div className="glass-card rounded-2xl p-5 border border-white/10">
            {rerunReady && (
              <ForjaYoloSetup
                onJobCreated={handleJobCreated}
                initialDatasetId={rerunDatasetId || undefined}
                initialParams={rerunParams}
                initialWeightsId={rerunWeightsId || undefined}
                initialOutputName={rerunOutputName || undefined}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
