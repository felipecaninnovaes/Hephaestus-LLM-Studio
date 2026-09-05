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

  if (checking) return <main className="studio-shell"><p>Verificando sessão…</p></main>;
  return (
    <main className="studio-shell">
      <span className="studio-badge">Sessão ativa</span>
      <h1>Hephaestus LLM Studio</h1>
      <button type="button" onClick={logout} disabled={leaving}>
        {leaving ? "Saindo…" : "Sair"}
      </button>
    </main>
  );
}
