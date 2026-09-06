"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { IconLock } from "@/components/icons";

type ApiError = { code?: string };

function messageFor(code: string | undefined): string {
  switch (code) {
    case "invalid_credentials":
      return "Senha incorreta.";
    case "setup_required":
      return "Servidor em modo setup — defina STUDIO_PASSWORD.";
    case "invalid_request":
      return "Envie a senha.";
    default:
      return "Falha inesperada.";
  }
}

export default function LoginPage() {
  const router = useRouter();
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Já logado (cookie válido) → volta à área logada; cookie ausente/
  // inválido → permanece no form (o middleware só checa existência).
  useEffect(() => {
    fetch("/api/auth/me", { credentials: "same-origin" }).then((res) => {
      if (res.ok) {
        router.replace("/");
      }
    }).catch(() => {});
  }, [router]);

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!password) {
      setError(messageFor("invalid_request"));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({ password }),
      });
      if (res.ok) {
        router.replace("/");
        router.refresh();
        return;
      }
      let code: string | undefined;
      try {
        code = ((await res.json()) as ApiError).code;
      } catch {
        code = undefined;
      }
      setError(messageFor(code));
    } catch {
      setError(messageFor(undefined));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="studio-shell relative overflow-hidden">
      {/* Luz óptica zenital decorativa de fundo (ref: v1/v2 LoginPage) */}
      <div className="absolute w-[500px] h-[500px] bg-emerald-500/10 rounded-full blur-[120px] pointer-events-none -top-24 -left-24" />
      <div className="absolute w-[400px] h-[400px] bg-cyan-500/5 rounded-full blur-[100px] pointer-events-none -bottom-20 -right-20" />

      <section
        className="glass-card w-[min(24rem,100%)] rounded-2xl px-8 py-8 text-left relative z-10 border border-white/10 shadow-2xl"
        aria-labelledby="login-title"
      >
        <div className="flex items-center justify-between">
          <span className="studio-badge">Hephaestus Studio</span>
          <span className="text-[10px] font-mono text-zinc-500">v1.3</span>
        </div>

        <h1 id="login-title" className="mt-4 mb-1 text-lg font-semibold text-zinc-100 flex items-center gap-2">
          <IconLock className="w-4 h-4" />
          <span>Entrar</span>
        </h1>
        <p className="mb-6 text-xs text-zinc-400">
          Acesso single-user deste studio local.
        </p>
        <form onSubmit={onSubmit} className="flex flex-col gap-4">
          <div>
            <label htmlFor="password" className="mb-1.5 block text-xs font-medium text-zinc-300">
              Senha
            </label>
            <input
              id="password"
              name="password"
              type="password"
              autoComplete="current-password"
              required
              placeholder="Digite sua senha…"
              autoFocus
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              disabled={submitting}
              className="w-full rounded-lg border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500 disabled:opacity-55"
            />
          </div>
          <p role="status" aria-live="polite" className="m-0 min-h-5 text-[11px] font-mono text-rose-400">
            {error}
          </p>
          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded-xl bg-emerald-500 px-4 py-3 text-sm font-semibold text-zinc-950 shadow-lg shadow-emerald-500/20 transition-all hover:bg-emerald-400 active:scale-[0.98] disabled:opacity-55 cursor-pointer"
          >
            {submitting ? "Entrando…" : "Entrar"}
          </button>
        </form>

        <div className="mt-6 pt-4 border-t border-white/5 flex items-center justify-between text-[11px] text-zinc-500 font-mono">
          <span>Single-User Mode</span>
        </div>
      </section>
    </main>
  );
}
