"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { IconDatabase } from "@/components/icons";
import type { ComponentType } from "react";

interface StudioTab {
  id: string;
  label: string;
  href: string;
  icon: ComponentType<{ className?: string }>;
}

const TABS: StudioTab[] = [
  { id: "datasets", label: "Datasets", href: "/datasets", icon: IconDatabase },
];

export default function TabsBar() {
  const pathname = usePathname();

  return (
    <nav
      aria-label="Módulos do studio"
      className="h-11 bg-zinc-950 border-b border-zinc-800/80 px-4 flex items-center"
    >
      <div
        role="tablist"
        aria-label="Módulos do studio"
        className="flex items-center space-x-1 overflow-x-auto"
      >
        {TABS.map((tab) => {
          const Icon = tab.icon;
          const isActive =
            pathname === tab.href || pathname.startsWith(`${tab.href}/`);
          return (
            <Link
              key={tab.id}
              href={tab.href}
              role="tab"
              aria-selected={isActive}
              className={`px-3 py-1.5 rounded-lg text-xs font-medium flex items-center space-x-2 transition-all whitespace-nowrap shrink-0 ${
                isActive
                  ? "bg-zinc-800/90 text-white border border-zinc-700/80 shadow-sm"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/60"
              }`}
            >
              <Icon />
              <span>{tab.label}</span>
            </Link>
          );
        })}
      </div>
    </nav>
  );
}
