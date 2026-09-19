"use client";

import { useEffect } from "react";
import { IconAlertTriangle, IconRefresh } from "@/components/icons";
import { Button } from "@/components/ui/Button";

export default function StudioError({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error("Studio runtime error:", error);
  }, [error]);

  return (
    <div className="flex h-full min-h-[60vh] flex-col items-center justify-center p-6 text-center">
      <div className="glass-card max-w-md rounded-2xl border border-rose-500/30 bg-rose-950/20 p-8 shadow-2xl backdrop-blur-md space-y-4">
        <div className="mx-auto flex size-12 items-center justify-center rounded-xl border border-rose-500/30 bg-rose-500/15 text-rose-400">
          <IconAlertTriangle className="size-6" />
        </div>

        <div className="space-y-2">
          <h2 className="font-display text-base font-bold text-white">
            Falha inesperada no Studio
          </h2>
          <p className="text-xs text-zinc-300 leading-relaxed">
            {error.message ||
              "Ocorreu um erro ao renderizar este módulo. Tente recarregar a visualização."}
          </p>
          {error.digest && (
            <p className="font-mono text-3xs text-zinc-500">
              Digest: {error.digest}
            </p>
          )}
        </div>

        <div className="pt-2 flex items-center justify-center gap-3">
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => window.location.reload()}
          >
            Recarregar Página
          </Button>
          <Button
            type="button"
            variant="primary"
            size="md"
            onClick={() => reset()}
            leftIcon={<IconRefresh className="size-4" />}
          >
            Tentar Novamente
          </Button>
        </div>
      </div>
    </div>
  );
}
