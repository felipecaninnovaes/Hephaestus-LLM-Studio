# AGENTS.md — Hephaestus LLM Studio (Nível 0: Macro)

Entrada canônica dos agentes (<130 linhas). Início de toda sessão: ler
`tasks/todo.md` (estado ativo) + este arquivo. Detalhes sob demanda em
`.agents/rules/` e `docs/REPO_MAP.md` (Nível 1).

## 1. Os Quatro Pilares (+ Contracts)

| Pilar | Diretório | Stack | Papel & Exposição |
| :--- | :--- | :--- | :--- |
| **Apps** | `apps/web/` | Next.js 16, TS, Tailwind v4 | UI do Studio (`:3000`); proxy `/api/*` → BFF |
| **Services** | `services/` | Rust (Axum, SQLx, Tokio) | `api-principal` (BFF público `:8080`), `manager` (`:8081`), `orchestrator` (`:8082`) |
| **Engines** | `engines/` | Python 3.11+, uv, PyTorch | `trainer-yolo`, `trainer-difusao`, `trainer-clip`. **Sem `ports:` no host** — isolamento por não-exposição |
| **Infra** | `infra/` | Docker Compose, scripts | Compose dev/gpu (Postgres+pgvector, SeaweedFS S3), runbooks, CI |
| **Contracts** | `packages/` | OpenAPI / YAML | `contracts/openapi.yaml` (wire `camelCase`), `policies/` (VRAM, engines) |

Chamada de engine **nunca** vem do browser: `api-principal → manager → orchestrator → engine`.

## 2. Comandos de Verificação (prefixar `rtk` p/ economia de tokens)

```bash
cargo check --workspace && cargo fmt --all -- --check && cargo test --workspace
npm run build --workspace=web && npm run lint --workspace=web   # raiz (workspaces)
python -m compileall engines/*/src
cd engines/<engine> && uv run pytest              # uv é POR ENGINE (não há projeto raiz)
docker compose -f infra/compose.yaml config -q
graft ask "<pergunta>" --source    # contexto cirúrgico ANTES de grep/leitura manual
graft build                        # reindexar após mudanças grandes
```

Dev é CPU-only: `ENGINE_MOCK=1`; testes `@gpu` são manuais, não rodar.

## 3. Regras Inegociáveis

1. **`main` protegida:** sem commits diretos; branches `feat/…`, `fix/…`,
   `chore/…`. Commits atômicos (100–300 LOC), Conventional Commits
   `tipo(escopo): descrição` em português. Push/merge só com ordem explícita.
2. **Git read-only p/ subagentes:** implementadores não commitem; o
   coordenador comita após verificação verde.
3. **Isolamento de engines:** proibido mapear `ports:` de engine para o host;
   todo acesso via cadeia de serviços.
4. **File Ownership:** paralelismo só com arquivos disjuntos; contratos
   (`packages/`, migrations, openapi) editados sequencialmente antes.
5. **Regra das Duas Correções:** 2 falhas do mesmo erro = contexto
   contaminado → parar, registrar em `tasks/todo.md`, escalar.
6. **Orçamento de contexto (funil L0→L3):** `AGENTS.md` → `docs/REPO_MAP.md` →
   `graft ask --source` / `graft skeleton` → leitura delimitada por offset.
   Proibido ler arquivos inteiros >100 linhas ou o histórico em
   `docs/archive/`. Zona saudável 15–35%; ao atingir ~40%: "Documentar e
   Limpar" (estado em `tasks/todo.md`, sessão nova).

## 4. Roteamento de Subagentes

| Tarefa | Agente | Modelo |
| :--- | :--- | :--- |
| Arquitetura, contratos OpenAPI/SQL, boundaries | `@architect` | caro |
| Rust (`api-principal`, `manager`, `orchestrator`) | `@rust-dev` | barato |
| Frontend Next.js (`apps/web`) | `@frontend-dev` | barato |
| Engines Python (`engines/*`, uv) | `@python-engines` | barato |
| Infra/CI (`infra/`, Dockerfiles, scripts) | `@infra-dev` | barato |
| Fixes mecânicos de build/lint (spec completa, máx. 2 tentativas) | `@fixer` | barato |
| Sincronizar `docs/` §9/§10 e contracts com o código | `@docs-sync` | barato |
| Mapeamento/Localização de código (read-only) | `@explore` | barato |
| Revisão de diff/contratos/boundary | `@reviewer` (escalada: `@reviewer-max`) | caro |
| Auditoria visual via Chrome DevTools MCP | `@ui-designer` | barato |
| Descrição factual de imagens/scrennshots | `@visao` | visão |
| Governança de arquitetura sob demanda | `@guardian` | barato |

Coordenação e decisão: `@hephaestus` (este agente). O coordenador **não
executa fixes mecânicos** — especifica e despacha.

## 5. Índice de Regras (Nível 1.5, carregar sob demanda)

- `.agents/rules/git.md` — branches, commits, atomicidade, permissões git.
- `.agents/rules/architecture.md` — pilares, boundaries, isolamento, schema.
- `.agents/rules/subagents.md` — hub-and-spoke, DAG, file ownership, relatórios.
- `.agents/rules/context-management.md` — funil L0→L3, orçamento, Documentar-e-Limpar.
- `.agents/rules/security.md` — segredos (.env), logs, auth.
- `.agents/rules/antigravity-rtk-rules.md` — filtro rtk em comandos shell.
- `docs/REPO_MAP.md` (L1) — portas, rotas, tabelas, módulos frontend.
- `tasks/todo.md` — estado ativo da sessão (memória do coordenador).
- `IDEIA.md` — intenção do produto (nunca contradizer em silêncio; expor conflitos).
