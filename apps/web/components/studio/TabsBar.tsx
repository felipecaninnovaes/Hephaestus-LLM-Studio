"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import type { ComponentType } from "react";
import {
  IconDatabase,
  IconFolder,
  IconImage,
  IconLayers,
  IconSparkles,
  IconTarget,
} from "@/components/icons";
import { listDatasets } from "@/lib/datasets";

interface StudioTab {
  id: string;
  label: string;
  badge?: string;
  href?: string;
  disabled?: boolean;
  icon: ComponentType<{ className?: string }>;
}

const TRAIN_TABS: StudioTab[] = [
  { id: "difusao", label: "Difusão", badge: "Flux·SDXL·1.5", disabled: true, icon: IconSparkles },
  { id: "openclip", label: "OpenCLIP", badge: "Embedding", disabled: true, icon: IconLayers },
  { id: "yolo", label: "YOLO", badge: "v8/v9/v11", disabled: true, icon: IconTarget },
];

const PREP_TABS: StudioTab[] = [
  { id: "autolabel", label: "AutoLabel", badge: "Difusão·CLIP", disabled: true, icon: IconImage },
  { id: "autotracker", label: "AutoTracker", badge: "Vídeo·Imagem", disabled: true, icon: IconFolder },
];

function DisabledTab({ tab }: { tab: StudioTab }) {
  const Icon = tab.icon;
  return (
    <span
      role="tab"
      aria-selected={false}
      aria-disabled={true}
      title="Disponível em fatia futura"
      className="flex shrink-0 cursor-not-allowed items-center space-x-2 rounded-lg px-3 py-1.5 text-xs font-medium whitespace-nowrap text-zinc-600 opacity-50 transition-all"
    >
      <Icon />
      <span>{tab.label}</span>
      {tab.badge && (
        <span className="rounded-full border border-zinc-800 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-[10px] text-zinc-600">
          {tab.badge}
        </span>
      )}
    </span>
  );
}

export default function TabsBar() {
  const pathname = usePathname();
  const [count, setCount] = useState<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    listDatasets()
      .then((items) => {
        if (!cancelled) setCount(items.length);
      })
      .catch(() => {
        if (!cancelled) setCount(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const datasetsActive =
    pathname === "/datasets" || pathname.startsWith("/datasets/");

  return (
    <nav
      aria-label="Módulos do studio"
      className="flex h-11 items-center border-b border-zinc-800/80 bg-zinc-950 px-4"
    >
      <div
        role="tablist"
        aria-label="Módulos do studio"
        className="flex items-center space-x-1 overflow-x-auto"
      >
        {TRAIN_TABS.map((tab) => (
          <DisabledTab key={tab.id} tab={tab} />
        ))}
        <span aria-hidden="true" className="mx-1 h-5 w-px shrink-0 bg-zinc-800" />
        {PREP_TABS.map((tab) => (
          <DisabledTab key={tab.id} tab={tab} />
        ))}
        <span aria-hidden="true" className="mx-1 h-5 w-px shrink-0 bg-zinc-800" />
        <Link
          href="/datasets"
          role="tab"
          aria-selected={datasetsActive}
          className={`flex shrink-0 items-center space-x-2 rounded-lg px-3 py-1.5 text-xs font-medium whitespace-nowrap transition-all ${
            datasetsActive
              ? "border border-zinc-700/80 bg-zinc-800/90 text-zinc-100 shadow-sm underline decoration-emerald-400 decoration-2 underline-offset-4"
              : "text-zinc-400 hover:bg-zinc-900/60 hover:text-zinc-200"
          }`}
        >
          <IconDatabase />
          <span>Datasets</span>
          {count !== null && (
            <span className="rounded-full border border-zinc-700/80 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-[10px] text-zinc-400">
              {count}
            </span>
          )}
        </Link>
      </div>
    </nav>
  );
}
