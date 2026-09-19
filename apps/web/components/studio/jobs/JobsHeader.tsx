"use client";

import Link from "next/link";
import {
  IconActivity,
  IconPlay,
  IconRefresh,
  IconTrash,
  IconZap,
} from "@/components/icons";
import { Button, getButtonClasses } from "@/components/ui/Button";
import { openActionCenter } from "@/lib/events";

interface JobsHeaderProps {
  totalCount: number;
  refreshing: boolean;
  onCleanup: () => void;
  onRefresh: () => void;
}

export function JobsHeader({
  totalCount,
  refreshing,
  onCleanup,
  onRefresh,
}: JobsHeaderProps) {
  return (
    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/10 pb-4">
      <div>
        <div className="flex items-center space-x-2.5">
          <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
            <IconActivity className="size-4" />
          </span>
          <h1 className="font-display text-lg font-bold text-white tracking-tight">
            Execuções
          </h1>
          {totalCount > 0 && (
            <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-2xs text-zinc-400 backdrop-blur-sm">
              {totalCount} {totalCount === 1 ? "execução" : "execuções"}
            </span>
          )}
        </div>
        <p className="mt-1 text-xs text-zinc-400 max-w-2xl">
          Fila de trabalho em execução e histórico de todos os tipos (YOLO,
          AutoTracker).
        </p>
      </div>

      <div className="flex items-center space-x-2.5 shrink-0">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={onCleanup}
          title="Limpar jobs antigos do histórico"
        >
          <IconTrash className="size-3.5 text-rose-400" />
          <span className="hidden sm:inline">Limpar antigos</span>
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={onRefresh}
          disabled={refreshing}
          title="Atualizar lista e status dos jobs"
        >
          <IconRefresh
            className={`size-3.5 ${refreshing ? "animate-spin text-brand-400" : ""}`}
          />
          <span>Atualizar</span>
        </Button>
        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={openActionCenter}
          title="Abrir Centro de Atividades lateral"
        >
          <IconZap className="size-3.5 text-brand-400" />
          <span className="hidden sm:inline">Centro de Atividades</span>
        </Button>
        <Link
          href="/treino"
          className={getButtonClasses({ variant: "primary", size: "sm" })}
        >
          <IconPlay className="size-3.5" />
          <span>Novo Treino</span>
        </Link>
      </div>
    </div>
  );
}
