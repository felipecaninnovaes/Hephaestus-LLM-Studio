"use client";

import { useMemo, useState } from "react";
import { IconImage, IconSparkles } from "@/components/icons";
import { EmptyState, SubmodulePills, type SubmodulePillItem } from "@/components/ui";
import GenerationPanel from "@/components/studio/GenerationPanel";

type GeracaoTab = "gerar" | "galeria";

/* ═══════════════════════════════════════════════════════════════════
   Página /geracao — Aba dedicada de geração de imagens (ADR-0023 D0/D6)
   Pills internas: Gerar | Galeria
   Breadcrumb: geracao → "Geração"
   ═══════════════════════════════════════════════════════════════════ */

export default function GeracaoPage() {
  const [activeTab, setActiveTab] = useState<GeracaoTab>("gerar");

  const tabPills = useMemo<SubmodulePillItem<GeracaoTab>[]>(
    () => [
      {
        id: "gerar",
        label: "Gerar",
        icon: <IconSparkles className="size-3.5 text-brand-400" />,
      },
      {
        id: "galeria",
        label: "Galeria",
        icon: <IconImage className="size-3.5 text-zinc-400" />,
      },
    ],
    [],
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {/* ── Topbar com título e pills ── */}
      <div className="shrink-0 border-b border-white/5 bg-zinc-950/40 px-4 py-3 md:px-6 backdrop-blur-md flex flex-wrap items-center justify-between gap-4">
        <div className="flex items-center space-x-2.5">
          <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
            <IconSparkles className="size-4" />
          </span>
          <div>
            <h1 className="font-display text-base md:text-lg font-bold text-white tracking-tight leading-none">
              Geração
            </h1>
            <p className="text-[11px] text-zinc-400 mt-0.5 font-mono">
              Ambiente de geração de imagens por difusão
            </p>
          </div>
        </div>

        <SubmodulePills<GeracaoTab>
          items={tabPills}
          value={activeTab}
          onChange={setActiveTab}
          size="sm"
        />
      </div>

      {/* ── Conteúdo ── */}
      <div className="flex-1 min-h-0 overflow-hidden">
        {activeTab === "gerar" ? (
          <GenerationPanel />
        ) : (
          /* ── Aba Galeria: placeholder honesto (G.8 implementa) ── */
          <div className="flex h-full items-center justify-center p-6">
            <EmptyState
              icon={<IconImage className="size-8 text-brand-400" />}
              title="Galeria de gerações"
              description="Disponível na próxima atualização."
            />
          </div>
        )}
      </div>
    </div>
  );
}
