"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";

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
    <main className="studio-shell">
      <section
        className="glass-card w-[min(24rem,100%)] rounded-2xl px-8 py-8 text-left"
        aria-labelledby="login-title"
      >
        <span className="studio-badge">Hephaestus Studio</span>
        <h1 id="login-title" className="mt-4 mb-1 text-lg font-semibold text-zinc-100">
          Entrar
        </h1>
        <p className="mb-6 text-xs text-zinc-400">
          Acesso single-user deste studio local.
        </p>
        <form onSubmit={onSubmit} className="flex flex-col gap-4">
          <div>
            <label htmlFor="password" className="mb-1 block text-xs font-medium text-zinc-300">
              Senha
            </label>
            <input
              id="password"
              name="password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              disabled={submitting}
              className="w-full rounded-lg border border-zinc-700/80 bg-zinc-900 px-3 py-2 font-mono text-xs text-zinc-200 disabled:opacity-55"
            />
          </div>
          <p role="status" aria-live="polite" className="m-0 min-h-5 text-[11px] font-mono text-rose-400">
            {error}
          </p>
          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded-xl bg-emerald-500 px-4 py-3 text-sm font-semibold text-zinc-950 shadow-lg shadow-emerald-500/20 transition-all hover:bg-emerald-400 active:scale-[0.98] disabled:opacity-55"
          >
            {submitting ? "Entrando…" : "Entrar"}
          </button>
        </form>
      </section>
    </main>
  );
}
