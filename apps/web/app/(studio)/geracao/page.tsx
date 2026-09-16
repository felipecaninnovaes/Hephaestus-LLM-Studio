"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { IconImage, IconSparkles } from "@/components/icons";
import { SubmodulePills, type SubmodulePillItem } from "@/components/ui";
import GenerationPanel from "@/components/studio/GenerationPanel";
import GenerationGallery from "@/components/studio/GenerationGallery";

type GeracaoTab = "gerar" | "galeria";

/* Key de persistência da aba ativa (Slice F1/001). Hidratação via
   useEffect para não causar hydration mismatch (SSR renderiza "gerar"). */
const GERACAO_TAB_KEY = "geracao:activeTab";

function readStoredTab(): GeracaoTab | null {
  try {
    const raw = window.localStorage.getItem(GERACAO_TAB_KEY);
    return raw === "gerar" || raw === "galeria" ? raw : null;
  } catch {
    return null;
  }
}

/* ═══════════════════════════════════════════════════════════════════
   Página /geracao — Aba dedicada de geração de imagens (ADR-0023 D0/D6)
   Pills internas: Gerar | Galeria
   Breadcrumb: geracao → "Geração"
   ═══════════════════════════════════════════════════════════════════ */

export default function GeracaoPage() {
  const [activeTab, setActiveTab] = useState<GeracaoTab>("gerar");

  /* ── Persiste + restaura a aba ativa (refresh não perde o contexto) ── */
  const handleTabChange = useCallback((tab: GeracaoTab) => {
    setActiveTab(tab);
    try {
      window.localStorage.setItem(GERACAO_TAB_KEY, tab);
    } catch {
      /* storage indisponível — aba segue viva só em memória */
    }
  }, []);

  useEffect(() => {
    const stored = readStoredTab();
    if (stored) setActiveTab(stored);
  }, []);

  /* ── Escuta evento customizado do empty state da galeria ── */
  useEffect(() => {
    function handleSwitchTab(e: Event) {
      const detail = (e as CustomEvent<GeracaoTab>).detail;
      if (detail === "gerar" || detail === "galeria") {
        handleTabChange(detail);
      }
    }
    window.addEventListener("hephaestus:switch-tab", handleSwitchTab);
    return () => window.removeEventListener("hephaestus:switch-tab", handleSwitchTab);
  }, [handleTabChange]);

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
    <div className="flex min-h-full flex-col lg:h-full lg:min-h-0 lg:overflow-hidden">
      {/* ── Topbar com título e pills ── */}
      <div className="shrink-0 border-b border-white/5 bg-zinc-950/40 px-4 py-3 md:px-6 backdrop-blur-md flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
        <div className="flex min-w-0 items-center space-x-2.5">
          <span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
            <IconSparkles className="size-4" />
          </span>
          <div>
            <h1 className="font-display text-base md:text-lg font-bold text-white tracking-tight leading-none">
              Geração
            </h1>
            <p className="text-2xs text-zinc-400 mt-0.5 font-mono">
              Ambiente de geração de imagens por difusão
            </p>
          </div>
        </div>

        <div className="w-full min-w-0 sm:w-auto">
          <SubmodulePills<GeracaoTab>
            items={tabPills}
            value={activeTab}
            onChange={handleTabChange}
            size="sm"
          />
        </div>
      </div>

      {/* ── Conteúdo ── */}
      <div className="flex-1 lg:min-h-0 lg:overflow-hidden">
        {activeTab === "gerar" ? (
          <GenerationPanel />
        ) : (
          <GenerationGallery />
        )}
      </div>
    </div>
  );
}
