"use client";

import { useRouter } from "next/navigation";
import {
  IconActivity,
  IconDatabase,
  IconServer,
  IconTarget,
} from "@/components/icons";

export interface ActionCenterEmptyStateProps {
  query: string;
  onClose: () => void;
}

export function ActionCenterEmptyState({
  query,
  onClose,
}: ActionCenterEmptyStateProps) {
  const router = useRouter();

  return (
    <div className="glass-card flex flex-col items-center gap-3.5 rounded-2xl p-8 text-center mt-6 border border-white/10">
      <span className="flex size-12 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
        <IconActivity className="size-6 text-brand-400" />
      </span>
      <div className="max-w-sm space-y-1">
        <h3 className="font-display text-sm font-semibold text-zinc-200">
          {query
            ? `Nenhum resultado para "${query}"`
            : "Nenhuma atividade recente"}
        </h3>
        <p className="text-xs text-zinc-400 leading-relaxed">
          O Centro de Atividades reúne as notificações do sistema em tempo real —
          treinos e tarefas de processamento, falhas de execução e alertas de
          recursos do nó (VRAM e CPU).
        </p>
      </div>

      {/* Central de Ações Rápidas do Estúdio */}
      <div className="mt-3 w-full grid grid-cols-1 sm:grid-cols-3 gap-2.5 pt-3 border-t border-white/5">
        <button
          type="button"
          onClick={() => {
            onClose();
            router.push("/datasets");
          }}
          className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
        >
          <IconDatabase className="size-4 text-brand-400" />
          <span className="text-xs font-medium text-zinc-200">Datasets</span>
          <span className="text-3xs text-zinc-400 font-mono">
            Gerenciar acervo
          </span>
        </button>

        <button
          type="button"
          onClick={() => {
            onClose();
            router.push("/jobs");
          }}
          className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
        >
          <IconTarget className="size-4 text-brand-400" />
          <span className="text-xs font-medium text-zinc-200">
            Forja do YOLO
          </span>
          <span className="text-3xs text-zinc-400 font-mono">
            Treino de visão
          </span>
        </button>

        <button
          type="button"
          onClick={() => {
            onClose();
            router.push("/dashboard");
          }}
          className="flex flex-col items-center gap-1.5 rounded-xl border border-white/10 bg-white/[0.02] p-3 text-center transition hover:border-brand-500/30 hover:bg-white/[0.06] cursor-pointer"
        >
          <IconServer className="size-4 text-brand-400" />
          <span className="text-xs font-medium text-zinc-200">
            Painel do Nó
          </span>
          <span className="text-3xs text-zinc-400 font-mono">
            Monitorar nós
          </span>
        </button>
      </div>
    </div>
  );
}
