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
        className="glass-card"
        aria-labelledby="login-title"
        style={{ padding: "2rem", width: "min(24rem, 100%)" }}
      >
        <span className="studio-badge">Hephaestus Studio</span>
        <h1 id="login-title" style={{ margin: "1rem 0 0.25rem" }}>
          Entrar
        </h1>
        <p style={{ margin: "0 0 1.5rem" }}>
          Acesso single-user deste studio local.
        </p>
        <form
          onSubmit={onSubmit}
          style={{ display: "flex", flexDirection: "column", gap: "0.75rem" }}
        >
          <label htmlFor="password" style={{ textAlign: "left" }}>
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
          />
          <p role="status" aria-live="polite" style={{ margin: 0, minHeight: "1.25rem" }}>
            {error}
          </p>
          <button type="submit" disabled={submitting}>
            {submitting ? "Entrando…" : "Entrar"}
          </button>
        </form>
      </section>
    </main>
  );
}
