"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";

export default function Home() {
  const router = useRouter();
  const [checking, setChecking] = useState(true);
  const [leaving, setLeaving] = useState(false);

  useEffect(() => {
    fetch("/api/auth/me", { credentials: "same-origin" }).then((res) => {
      if (res.ok) setChecking(false);
      else router.replace("/login");
    }).catch(() => router.replace("/login"));
  }, [router]);

  async function logout() {
    setLeaving(true);
    await fetch("/api/auth/logout", { method: "POST", credentials: "same-origin" });
    router.replace("/login");
    router.refresh();
  }

  if (checking)
    return (
      <main className="studio-shell">
        <p className="font-mono text-xs text-zinc-400">Verificando sessão…</p>
      </main>
    );
  return (
    <main className="studio-shell">
      <span className="studio-badge">Sessão ativa</span>
      <h1 className="text-zinc-100">Hephaestus LLM Studio</h1>
      <p>As telas do studio chegam nas próximas fatias.</p>
      <button
        type="button"
        onClick={logout}
        disabled={leaving}
        className="rounded-lg border border-zinc-700/80 bg-zinc-900/60 px-4 py-2 text-xs font-medium text-zinc-200 transition-colors hover:bg-zinc-800 disabled:opacity-55"
      >
        {leaving ? "Saindo…" : "Sair"}
      </button>
    </main>
  );
}
