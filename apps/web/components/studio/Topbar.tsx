"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { IconLogOut, IconTarget } from "@/components/icons";

export default function Topbar() {
  const router = useRouter();
  const [leaving, setLeaving] = useState(false);

  async function logout() {
    setLeaving(true);
    try {
      await fetch("/api/auth/logout", {
        method: "POST",
        credentials: "same-origin",
      });
    } catch {
      // Mesmo sem resposta, a sessão local termina aqui.
    }
    router.replace("/login");
    router.refresh();
  }

  return (
    <header className="h-14 border-b border-zinc-800/80 bg-zinc-950/80 backdrop-blur-xl px-4 flex items-center justify-between sticky top-0 z-40">
      <div className="flex items-center space-x-4">
        <div className="flex items-center space-x-3">
          <div className="w-8 h-8 rounded-lg bg-emerald-500/10 border border-emerald-500/30 flex items-center justify-center text-emerald-400 shadow-sm shadow-emerald-500/10">
            <IconTarget />
          </div>
          <div className="flex items-center space-x-2">
            <span className="font-semibold text-sm tracking-tight text-white">
              Hephaestus Studio
            </span>
            <span className="studio-badge text-[10px] py-0.5 px-2">
              v0.3
            </span>
          </div>
        </div>

        <div className="h-4 w-px bg-zinc-800" aria-hidden="true"></div>

        <span className="flex items-center space-x-2 px-2.5 py-1.5 rounded-lg bg-zinc-900/90 border border-zinc-800 text-xs font-medium text-zinc-300">
          <span className="w-2 h-2 rounded-full bg-emerald-400"></span>
          <span className="font-mono text-zinc-200 hidden sm:inline">
            Docker Local
          </span>
        </span>
      </div>

      <div className="flex items-center">
        <button
          type="button"
          onClick={logout}
          disabled={leaving}
          aria-label="Sair do studio"
          className="flex items-center space-x-1.5 px-2.5 py-1.5 rounded-lg text-xs text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/60 transition-colors disabled:opacity-55"
        >
          <IconLogOut className="w-3.5 h-3.5" />
          <span>{leaving ? "Saindo…" : "Sair"}</span>
        </button>
      </div>
    </header>
  );
}
