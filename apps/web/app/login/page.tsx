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
    <main className="studio-shell relative overflow-hidden bg-[var(--bg)]">
      {/* Luz óptica zenital decorativa de fundo com undertone Arcane Violet */}
      <div aria-hidden="true" className="absolute w-[520px] h-[520px] bg-brand-500/10 rounded-full blur-[130px] pointer-events-none -top-24 -left-24" />
      <div aria-hidden="true" className="absolute w-[440px] h-[440px] bg-brand-700/10 rounded-full blur-[110px] pointer-events-none -bottom-20 -right-20" />

      <section
        className="glass-card w-[min(25rem,100%)] rounded-2xl px-8 py-8 text-left relative z-10 border border-brand-500/20 shadow-2xl"
        aria-labelledby="login-title"
      >
        <div className="flex items-center justify-between">
          <span className="studio-badge">Hephaestus Studio</span>
          <span className="text-[10px] font-mono text-zinc-500">v1.3</span>
        </div>

        <h1 id="login-title" className="font-display mt-4 mb-1 text-lg font-semibold text-zinc-100 flex items-center gap-2">
          <IconLock className="w-4 h-4" />
          <span>Bem-vindo(a) de volta</span>
        </h1>
        <p className="mb-6 text-xs text-zinc-400">
          Acesso single-user deste studio local.
        </p>
        <form onSubmit={onSubmit} className="flex flex-col gap-4">
          <div>
            <label htmlFor="password" className="mb-1.5 block font-mono text-[11px] font-medium uppercase tracking-[0.08em] text-zinc-400">
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
              className="w-full min-h-[44px] rounded-xl border border-zinc-700/80 bg-zinc-900 px-3.5 py-2.5 font-mono text-base sm:text-xs text-zinc-200 placeholder:text-zinc-600 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 disabled:opacity-55 touch-manipulation"
            />
          </div>
          <p role="status" aria-live="polite" className="m-0 min-h-5 text-[11px] font-mono text-red-500">
            {error}
          </p>
          <button
            type="submit"
            disabled={submitting}
            className="h-11 w-full rounded-xl bg-brand-500 px-4 text-sm font-semibold text-white shadow-lg shadow-brand-500/20 transition-all hover:bg-brand-600 active:scale-[0.98] disabled:opacity-55 cursor-pointer"
          >
            {submitting ? "Autenticando…" : "Entrar no Hephaestus"}
          </button>
        </form>

        <div className="mt-6 pt-4 border-t border-white/5 font-mono text-[11px] text-zinc-500">
          <span>Single-User Local</span>
        </div>
      </section>
    </main>
  );
}
