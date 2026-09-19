"use client";

import { useEffect, useMemo, useState } from "react";
import { getTelemetry } from "@/lib/jobs";
import type { Telemetry } from "@/types/studio";

/**
 * Hook de telemetria de hardware com polling consciente de visibilidade da página.
 * Fornece estado de hardware, capacidade de VRAM em GB e rótulo do dispositivo.
 */
export function useHardwareTelemetry(pollingInterval = 10000) {
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadTelem() {
      if (
        typeof document !== "undefined" &&
        document.visibilityState === "hidden"
      ) {
        return;
      }
      try {
        const t = await getTelemetry();
        if (!cancelled) setTelemetry(t);
      } catch {
        // Best-effort
      }
    }

    void loadTelem();
    const timer = setInterval(loadTelem, pollingInterval);

    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        void loadTelem();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      cancelled = true;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [pollingInterval]);

  const nodeVramTotalGb = useMemo(() => {
    if (telemetry?.vramTotal && telemetry.vramTotal > 0) {
      return telemetry.vramTotal > 1000
        ? Math.round((telemetry.vramTotal / (1024 * 1024 * 1024)) * 10) / 10
        : telemetry.vramTotal;
    }
    return null;
  }, [telemetry?.vramTotal]);

  const deviceLabel = useMemo(() => {
    if (telemetry?.gpus && telemetry.gpus.length > 0) {
      return `${telemetry.gpus[0]} (${nodeVramTotalGb || 24} GB)`;
    }
    return "Host CPU (Modo Mock)";
  }, [telemetry?.gpus, nodeVramTotalGb]);

  return {
    telemetry,
    nodeVramTotalGb,
    deviceLabel,
  };
}
