# ADR-0001 — Auth single-user (Fatia 2)

- **Status:** Aceito
- **Data:** 2026-09-04
- **Componentes:** `services/api-principal`, `packages/contracts`, Postgres (`db`)
- **Fontes:** `docs/backend.md` §2 (comportamento), §9 (paths), §10 (schema), §11 (setup);
  `docs/frontend.md` §10 + tela `/login`; `docs/repo-estrutura.md` (fatia < 400 linhas, snapshot OpenAPI)
- **Contrato:** `packages/contracts/openapi.yaml` (semeado junto com este ADR)

## Contexto

O api-principal é hoje um axum 0.7 mínimo (`/health` apenas, sem driver de banco).
A Fatia 2 entrega o primeiro slice vertical real: contrato → migration → endpoint → teste,
limitado ao domínio auth. Modelagem: **single-user** — exatamente 1 linha em `users`,
sem RBAC, sem multi-tenant; senha definida por `STUDIO_PASSWORD` no primeiro boot (§2, §11).
Front nunca fala com manager/orchestrator, e este slice não toca neles.

## Decisões

### D1 — Hash de senha: Argon2id (`argon2 = "0.5"`, params default do crate)
`Params::default()` do argon2-0.5 = **m=19 MiB, t=2, p=1** (OWASP 2024) ≈ 30–50 ms em
CPU modesta — o mínimo recomendado mesmo para um só verificador/serviço. Formato PHC
(`argon2id$v=19$m=...$t=...$p=...$salt$hash`) string inteiro em `users.password_hash`
(coluna TEXT — autodescritiva, permite recomputar parâmetros no `reset-password` futuro).
Salt 16 B aleatório via `OsRng` (rand_core 0.6).
**Descartado: bcrypt** — `backend.md` §11 já prescreve Argon2; bcrypt ainda traz limite de
72 bytes e memory-hardness menor sem ganho neste contexto.

### D2 — JWT: `jsonwebtoken = "9"`, HS256, TTL 7 dias, sem refresh
- **Claims:** `iss: "hephaestus-studio"` · `sub` = UUID do user (string) · `iat` ·
  `exp = iat + 7d` · `jti` = hex aleatório de 128 bits (log/auditoria; sem blacklist na v1).
  Validação com `leeway = 30 s` (drift de relógio em laptop suspenso).
- **Por que JWT e não sessão em tabela:** §2 nomeia JWT explicitamente; sem estado de
  sessão no Postgres, o restart do principal não derruba o usuário do studio — propriedade
  que sobrevive a todas as fatias seguintes.
- **Cookie:** `heph_session` — `HttpOnly; SameSite=Lax; Path=/; Max-Age=604800`.
  `Secure` somente sob TLS, via env `SECURE_COOKIE=true` (dev não tem TLS). `SameSite=Lax`
  é suficiente porque o front é same-site via `NEXT_PUBLIC_API_URL` (cross-origin porta
  3000→8080: o cookie **requer** que o web configure `credentials: 'include'`/`proxy` —
  ver Tensões/T5).
- **Segredo (a decisão difícil):** gerado no primeiro boot (32 B `OsRng`) e persistido no
  Postgres em tabela única `auth_state` (linha única, D4), lido antes de emitir o 1º token.
  Env `AUTH_SECRET` (hex 64 chars) **sobrepõe** quando presente. Consequência: trocar o
  segredo (editar a linha / setar env) derruba todas as sessões — em single-user isso é um
  bugfix, não um problema.
  **Descartados:** (a) `AUTH_SECRET` obrigatório — quebra o "compose up funciona out of
  box" e adiciona fricção no fluxo principal; (b) salvar em `settings` — fora do slice e
  exige cifra app-level com `STUDIO_MASTER_KEY` (§11), preocupação de outra fatia;
  (c) arquivo em disco — principal não tem volume persistente no compose hoje; segredo
  morreria a cada rebuild de container e sessões cairiam junto.
  Custo aceito: o segredo dorme em claro no Postgres local, protegido pela mesma fronteira
  de host que a senha de `DATABASE_URL` —threat model local (D7).
- **`/api/auth/me` retorna** `{ userId: sub, loggedAt: data_time(iat) }` — `iat` como
  "loggedAt" é estável entre chamadas e igual para toda a sessão.

### D3 — Bootstrap primeiro boot: fail-closed com estado visível (proposta validada + 2 ajustes)
Ordem no startup do principal: conectar pool → **rodar migrations** → carregar/gerar segredo
→ bootstrap de usuário:

| Condição (`users` vazio?) | `STUDIO_PASSWORD` | Comportamento |
|---|---|---|
| sim | definido (não-vazio) | `INSERT ... WHERE NOT EXISTS` com hash Argon2 → modo normal |
| sim | ausente/vazio | **modo setup**: `/health` 200 com `auth: "setup_required"`; rotas protegidas 401; `POST login` 503 |
| não | qualquer | env **ignorado** (troca de senha só via CLI `studio reset-password` — §11; fora do slice, ver T4) |

Ajustes sobre a proposta: (1) em modo setup o login responde **503** `setup_required`,
não 401 — indisponibilidade ≠ credencial, e o operador recebe pista de diagnóstico sem
vazar nada sobre credenciais (não existe usuário para vazar); (2) a contagem de linhas é
dispensável — `INSERT … ON CONFLICT DO NOTHING … WHERE NOT EXISTS (SELECT 1 FROM users)`
é atômico e basta (instância única; compose não escala principal).
**Descartado:** panicar no boot sem senha — transformaria o primeiro `docker compose up`
em crash-loop sem mensagem legível; fail-closed + `/health` informativo é estritamente melhor.

### D4 — Migration: `sqlx 0.8` + `sqlx::migrate!("./migrations")`
Embedding em tempo de compilação → **sem** `DATABASE_URL` no build (só runtime).
`services/api-principal/migrations/0001_users.sql`:

```sql
-- Fatia 2 / ADR-0001. Compatível com backend.md §10.
-- gen_random_uuid() é NATIVO desde o PG13 (postgres:16) — NÃO requer pgcrypto.
CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Adição ao §10 (ADR-0001 T1): segredo HS256 persistido no Postgres, linha única.
CREATE TABLE auth_state (
    id         SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    jwt_secret BYTEA NOT NULL CHECK (octet_length(jwt_secret) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```
**Descartado:** diesel + migrations próprias — segundo stack de DB no workspace por zero
ganho; sqlx já é o motor das fatias de datasets/jobs.

### D5 — Logout: limpar cookie, sem blacklist de `jti` na v1
JWT stateless permanece válido até `exp` (≤ 7 d) mesmo após logout. Risco aceitável:
atingir um token roubado exige acesso ao host/DevTools do único usuário — quem tem isso já
tem a senha. **Descartado:** blacklist (falsa segurança que morre no restart) e tabela de
sessões no Postgres (estado novo que a fatia não pediu). *Revisitar quando houver exposição
remota (Fatia 8+, junto com TLS).*

### D6 — Erros: envelope único `{code, message}`
`code` ∈ `invalid_request | invalid_credentials | unauthorized | setup_required | internal`
(estável, a UI ramifica por ele); `message` estático por código, nunca contém hash, token,
segredo nem existência de usuário. 401 do gate é genérico; 404 de rota inexistente só para
sessão válida (D9).

### D7 — Rate-limit de login: NENHUM na v1 — risco registrado
Loopback local (§2) não tem adversário de rede. **Risco aceito:** quem publicar a 8080 em
LAN/VPN sem reverse-proxy fica exposto a brute-force; `STUDIO_PASSWORD` default do compose
é `changeme`, então a mitigação real hoje é operacional (não expor). O contrato reserva
`429` com `x-reserved: true` para uma v2 LAN sem quebra de shape; implementar com
`tower`-throttle (5 falhas/min por IP) quando o cenário surgir.

### D8 — teste de contrato: snapshot da OpenAPI vs router, sem codegen
`tests/contract.rs` no api-principal (roda em `cargo test`, sem Postgres, sem Docker):
1. **Inventário:** as rotas do app nascem de uma tabela declarativa
   (`auth::routes()` → lista de `(method, path, status_codes)`); o teste faz o parse de
   `packages/contracts/openapi.yaml` com `serde_yaml` e compara os dois conjuntos
   `(path, method, status)` — ignorando entradas `x-reserved: true`.Qualquer deriva = teste vermelho.
2. **Sonda de invariantes:** `tower::ServiceExt::oneshot` no router com estado "setup"
   (pool ausente nunca é tocado nesses casos): `/health`→200 `auth:setup_required`;
   `GET /api/auth/me` sem cookie→401; `POST /api/auth/logout`→204;
   `POST /api/auth/login {}`→400; rota protegida inexistente sem cookie→401 (não 404, D9).
3. Caminho feliz com banco (login 200 + cookie, me 200, gate 401 com cookie válido,
   expiração) fica no **integ** (`compose.integ` com `db`, `scripts/e2e-smoke.sh`).
**Descartado:** utoipa code-first — inverteria a regra do repo (contrato é a fonte, o código
prova conformidade) e o gerador não expressa bem cookie auth + `x-reserved`.
`packages/contracts/openapi.yaml` também alimenta geração de tipos TS do front quando a
tela `/login` chegar (fatia UI própria).

### D9 — Gate de auth em axum 0.7: `route_layer` + fallback
`middleware::from_fn_with_state(require_auth)` aplicado com `.route_layer()` a um router
protegido, isentando por prefixo `/api/auth/*` e `/health` (letra exata do §2). Gotcha do
axum 0.7: `.layer()` **não** cobre caminhos não roteados (caem no 404 cru do matchit).
Regra fechada:
- rota existente sem cookie válido → `401 {"code":"unauthorized"}` (gate);
- rota **inexistente** sem cookie válido → também `401` via `.fallback(require_auth_fallback)`
  — sonda autenticada não distingue rotas existentes de inexistentes (não-divulgação);
- rota inexistente **com** sessão válida → `404` sem body (comportamento de roteamento,
  deliberadamente fora do envelope `{code,message}` e fora da OpenAPI desta fatia).

Além disso `/api/auth/me` fica fora do `require_auth` (gate por prefixo, §2) e **valida o
próprio cookie no handler** — não é brecha: é o gate daquela rota, e mantém §2 verdadeiro
ao pé da letra (T2).

Implementação: `route_layer` adiado p/ fatia 3 (axum 0.7.9 panic em router vazio, ver `routes.rs`);
fail-closed garantido pelo fallback; `PROTECTED_ROUTES` é o cinto estrutural.

## Consequências

- **Novas dependências (Cargo.toml do api-principal), versões fixas:**
  ```toml
  sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-rustls", "postgres", "uuid", "chrono", "migrate"] }
  argon2 = "0.5"
  rand_core = { version = "0.6", features = ["getrandom"] }   # OsRng p/ argon2 0.5 + segredo
  jsonwebtoken = "9"
  uuid = { version = "1", features = ["v4", "serde"] }
  chrono = { version = "0.4", features = ["serde"] }
  serde = { version = "1", features = ["derive"] }
  # dev-dependencies (teste de contrato D8):
  tower = { version = "0.4", features = ["util"] }
  http = "1"
  http-body-util = "0.1"
  serde_yaml = "0.9"
  ```
  (`reqwest`/`utoipa`/`time`-crate: proibidos nesta fatia.)
- **Env vars:** `DATABASE_URL` (passa a ser lida de fato), `STUDIO_PASSWORD` (só no 1º boot),
  `AUTH_SECRET` (opcional, override), `SECURE_COOKIE` (opcional, default `false`).
  compose já fornece os dois primeiros; nenhum novo volume necessário (D2 elimina a mudança
  de infra que segredo-em-arquivo exigiria).
- O startup do principal passa a depender do Postgres (pool + migrations). É aceitável pelo
  §1 ("dono da verdade") e pelo `depends_on: db` do compose; o `/health` só responde depois
  de as migrations terem corrido (sem migrations = serviço não está vivo de verdade).
- Front: nenhum código TS neste slice. Contrato já suporta a tela `/login` da fatia de UI
  (camelCase + cookie; lembrete do `credentials:'include'` registrado na D2).

## Tensões nos docs (expostas, não resolvidas em silêncio)

- **T1 — `auth_state` fora do §10.** A tabela do segredo não consta do schema canônico;
  §10 precisa ganhar a linha `auth_state(id smallint PK, jwt_secret BYTEA, created_at)` em
  doc-follow-up (edição de `backend.md` fora do escopo desta fatia de 2 arquivos).
- **T2 — §2 vs `/me`.** "Middleware em tudo exceto `/api/auth/*`" deixa `/me` isento do
  gate; a saída (D9: handler valida o próprio cookie) preserva as duas frases do doc.
- **T3 — casing.** `LoginRequest`/`MeResponse` em camelCase (`userId`, `loggedAt`) conforme
  contrato; §9 usa snake_case nos bodies de settings (`hf_token`…). Definir a política
  global de casing **antes** da Fatia 3 (datasets) e registrar em `backend.md`.
- **T4 — `studio reset-password` não existe.** §11 o cita; sem ele a senha fica gravada na
  primeira definição. Aceito para v1 (single-user local); fatia CLI própria depois —
  não entra aqui para não estourar os <400 linhas.
- **T5 — cross-origin 3000→8080.** `localhost:3000` e `localhost:8080` são *cross-origin*
  p/ o navegador: o front terá de usar `credentials: 'include'` e o principal precisará de
  `Access-Control-Allow-Origin` estrito + `Allow-Credentials` (ou proxy Next para `/api`).
  Isso não é mudança de contrato auth, mas é pré-condição da tela `/login`; registrar na
  fatia de UI.
- **T6 — default `changeme` no compose** convive com D7 (sem rate-limit): risco aceito e
  documentado; mitigar na fatia de docs de deploy.

## Plano de fatia (para despachar; cada passo é um commit < 400 linhas)

### Fatia 2A — migração + segredo + handlers auth (~300 linhas)
| # | Arquivo-alvo | O que | Critério de aceite |
|---|---|---|---|
| 2A.1 | *(este commit)* `packages/contracts/openapi.yaml` + ADR | contrato semente | YAML parseia; paths ≡ §9 |
| 2A.2 | `services/api-principal/migrations/0001_users.sql` | DDL D4 (users + auth_state) | `sqlx migrate run` aplica no compose-db |
| 2A.3 | `services/api-principal/Cargo.toml` | deps exatas de D2/consequências | `cargo check -p api-principal` |
| 2A.4 | `src/main.rs` | pool `PgPool` via `DATABASE_URL`, `sqlx::migrate!`, state `App { pool, secret, secure_cookie }`, listener 8080 | sobe com `db` do compose; `/health` inclui `auth` |
| 2A.5 | `src/auth/mod.rs` | `pub mod password, secret, session, handlers, routes;` + `App` state | compila |
| 2A.6 | `src/auth/password.rs` | `hash()`/`verify()` argon2id + `ensure_bootstrap_user()` (D3, INSERT atômico) | unit: hash→verify true; verify senha errada false |
| 2A.7 | `src/auth/secret.rs` | `load_or_generate_secret()` (env `AUTH_SECRET` > `auth_state` > INSERT gerado) | unit: shape 32 B; sem DB retorna erro (não pânico) |
| 2A.8 | `src/auth/session.rs` | `issue_jwt(user_id, secret) -> (String, iat)`; `verify_jwt(token, secret) -> Claims` | unit: válido OK; expirado → err; tampered → err |
| 2A.9 | `src/auth/handlers.rs` | login/me/logout (D6 envelope; login 200/400/401/503; me self-validado; logout 204 + Set-Cookie limpo) | manual `curl` com `STUDIO_PASSWORD` no compose: fluxo completo |

### Fatia 2B — gate + teste de contrato (~180 linhas)
| # | Arquivo-alvo | O que | Critério de aceite |
|---|---|---|---|
| 2B.1 | `src/auth/routes.rs` | tabela declarativa de rotas (D8) + montagem `Router` (protegido com `require_auth` route_layer + fallback D9; `/api/auth/*` fora; `/health` fora) | — |
| 2B.2 | `src/auth/gate.rs` | `require_auth` (Bearer? não: cookie `heph_session` → `verify_jwt` → insert `AuthUser` no request ext) | unit: cookie inválido 401 |
| 2B.3 | `tests/contract.rs` | mecanismo D8 (YAML ≡ inventário + sondas oneshot) | `cargo test -p api-principal` verde; remover uma rota do código quebra o teste |
| 2B.4 | `scripts/e2e-smoke.sh` (estender) | fluxo auth contra `compose.integ`: setup_required → login → me → logout → me 401 | smoke passa |

**Definition of Done da Fatia 2:** `cargo test -p api-principal` (unit + contract) e
`bash scripts/e2e-smoke.sh` verdes; Nenhuma tabela de `settings`/jobs tocada; diff de cada
commit < 400 linhas; sem mudança em manager/orchestrator/engines.

## Riscos e o que testar

- **Derrapada de fuso/hora do laptop** → token "expirado" subitamente: testar `leeway` de
  30 s e UX de re-login (front, fatia própria).
- **Regeneração acidental do segredo** (banco novo + env `AUTH_SECRET` esquecido) = todas
  as sessões caem: coberto pela sonda de `/health auth` state.
- **Corrida no bootstrap** com dois processos apontados no mesmo DB: mitigado pelo INSERT
  atômico; não testar concorrência (single-instance por construção).
- **Cookie em cross-origin** (T5): só explode na fatia do front — deixar a sonda do ADR lá.
- **Contrato vs deriva de código**: o teste D8.1 é a proteção primária; rodar em CI (já há
  lefthook/commitlint; adicionar `cargo test` no pre-push se ainda não houver).
