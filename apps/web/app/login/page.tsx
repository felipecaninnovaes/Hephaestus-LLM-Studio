"use client";

import { useEffect, useState, type FormEvent } from "react";
import { useRouter } from "next/navigation";
import { IconLock } from "@/components/icons";
import { Button } from "@/components/ui/Button";

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
    <>
      {/* ── AuthAmbient background (Arcane v2.1) ── */}
      <style>{`
        @keyframes rise {
          from { opacity: 0; transform: translateY(12px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @keyframes spin {
          from { transform: translate(-50%, -50%) rotate(0deg); }
          to   { transform: translate(-50%, -50%) rotate(360deg); }
        }
        @media (prefers-reduced-motion: reduce) {
          .rise { animation: none !important; opacity: 1 !important; }
          .ambient-shimmer { animation: none !important; }
        }
      `}</style>

      {/* Camada fixa de fundo com as 4 texturas */}
      <div
        aria-hidden="true"
        className="pointer-events-none fixed inset-0 overflow-hidden"
        style={{ background: "var(--bg)" }}
      >
        {/* 1 — Mesh: 4 radial-gradients violeta */}
        <div
          className="absolute inset-0"
          style={{
            background:
              "radial-gradient(ellipse 65% 50% at 18% 22%, rgba(131,80,242,0.22), transparent 65%)," +
              "radial-gradient(ellipse 60% 45% at 82% 78%, rgba(131,80,242,0.20), transparent 65%)," +
              "radial-gradient(ellipse 50% 55% at 78% 18%, rgba(131,80,242,0.18), transparent 60%)," +
              "radial-gradient(ellipse 55% 40% at 22% 82%, rgba(131,80,242,0.16), transparent 65%)",
          }}
        />

        {/* 2 — Grid: SVG pattern 48×48 contínuo */}
        <div
          className="absolute inset-0"
          style={{
            opacity: 0.75,
            backgroundImage:
              'url("data:image/svg+xml,%3Csvg xmlns=\'http://www.w3.org/2000/svg\' width=\'48\' height=\'48\'%3E%3Cpath d=\'M47.5 0v48M0 47.5h48\' stroke=\'rgba(160,150,185,1)\' stroke-width=\'1\' stroke-opacity=\'0.18\' fill=\'none\'/%3E%3C/svg%3E")',
            backgroundSize: "48px 48px",
          }}
        />

        {/* 3 — Noise: feTurbulence fractalNoise */}
        <div
          className="absolute inset-0"
          style={{
            opacity: 0.05,
            backgroundImage:
              'url("data:image/svg+xml;utf8,<svg xmlns=\'http://www.w3.org/2000/svg\' viewBox=\'0 0 200 200\'><filter id=\'n\'><feTurbulence type=\'fractalNoise\' baseFrequency=\'0.9\' numOctaves=\'2\' stitchTiles=\'stitch\'/><feColorMatrix values=\'0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0\'/></filter><rect width=\'100%25\' height=\'100%25\' filter=\'url(%23n)\'/></svg>")',
          }}
        />

        {/* 4 — Vignette */}
        <div
          className="absolute inset-0"
          style={{
            background:
              "radial-gradient(ellipse at center, transparent 40%, color-mix(in oklab, var(--bg) 80%, transparent) 100%)",
          }}
        />

        {/* 5 — Shimmer cónico rotativo (opcional, Arcane) */}
        <div
          className="ambient-shimmer absolute top-1/2 left-1/2 w-[300vmax] h-[300vmax]"
          style={{
            animation: "spin 60s linear infinite",
            background:
              "conic-gradient(from 0deg, rgba(131,80,242,0.08), rgba(131,80,242,0.05), rgba(131,80,242,0.07), rgba(131,80,242,0.08))",
          }}
        />
      </div>

      {/* ── Conteúdo centralizado ── */}
      <main className="relative z-10 flex min-h-dvh items-center justify-center p-6">
        <div className="flex w-full max-w-[400px] flex-col items-center">
          {/* Logo / Identidade */}
          <div
            className="rise flex flex-col items-center gap-2"
            style={{ animationDelay: "0ms" }}
          >
            <span
              className="inline-flex items-center gap-2 font-display text-xl font-semibold text-zinc-100"
              style={{
                filter: "drop-shadow(0 0 28px rgba(131,80,242,0.45))",
              }}
            >
              <IconLock className="size-5" />
              Hephaestus Studio
            </span>
            <span className="font-mono text-[10px] tracking-[0.2em] uppercase text-zinc-400/60">
              v1.3
            </span>
          </div>

          {/* Panel de login */}
          <section
            className="rise relative mt-10 w-full overflow-hidden rounded-2xl border border-white/10 bg-[rgba(32,32,38,0.40)] p-6 backdrop-blur-xl sm:p-8"
            aria-labelledby="login-title"
            style={{ animationDelay: "150ms" }}
          >
            {/* Hairline violeta no topo */}
            <span
              aria-hidden="true"
              className="absolute top-0 left-6 right-6 h-px"
              style={{
                background:
                  "linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)",
              }}
            />

            <h1
              id="login-title"
              className="text-center font-display text-2xl font-semibold tracking-tight text-zinc-100"
            >
              Bem-vindo(a) de volta
            </h1>
            <p className="mt-1.5 text-center text-sm text-zinc-400">
              Acesso single-user deste studio local.
            </p>

            <form onSubmit={onSubmit} className="mt-6 space-y-4">
              {/* Campo senha com ícone à esquerda */}
              <div>
                <label
                  htmlFor="password"
                  className="mb-1.5 block text-xs text-zinc-300"
                >
                  Senha
                </label>
                <div className="relative">
                  <IconLock className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-zinc-500" />
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
                    className="h-10 w-full rounded-lg border border-white/10 bg-white/[0.04] pl-9 pr-3 text-[16px] sm:text-sm text-zinc-100 placeholder:text-zinc-500 backdrop-blur-sm transition focus:border-brand-500/50 focus:ring-2 focus:ring-brand-500/30 focus:outline-none disabled:opacity-55"
                  />
                </div>
              </div>

              {/* Mensagem de erro */}
              <p
                role="status"
                aria-live="polite"
                className="m-0 min-h-5 text-sm text-[#ef4444]"
              >
                {error}
              </p>

              {/* CTA primário outline-translúcido */}
              <Button
                type="submit"
                variant="primary"
                size="lg"
                loading={submitting}
                className="mt-1 w-full"
              >
                {submitting ? "Autenticando…" : "Entrar no Hephaestus"}
              </Button>
            </form>
          </section>

          {/* Footer */}
          <div
            className="rise mt-8 text-center"
            style={{ animationDelay: "300ms" }}
          >
            <span className="font-mono text-[10px] tracking-[0.2em] uppercase text-zinc-400/60">
              Single-User Local
            </span>
          </div>
        </div>
      </main>
    </>
  );
}
