# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/wave-2-execucao-orquestracao`
- **Fatia em andamento:** Wave 2 — Camada Interna de Execução e Orquestração (`tasks/consolidacao-auditoria-roadmap.md`).

## Checklist Imediato da Sessão Ativa (Wave 2)

- [x] RD-020: Repasse de `control_package_ref` no dispatch do manager.
- [x] RD-021: Ajuste semântico de abort (`cancelled` vs `failed`) no orchestrator.
- [x] RD-022: Unificação de leitura de telemetria (`telemetry.jsonl`) no orchestrator.
- [x] RD-023: Execução de containers como usuário não-root (UID 1000).
- [x] Verificação e testes da Wave 2.
## Entregas Concluídas Recentemente
- [x] Wave 0: Fundação de contratos OpenAPI, normalização de políticas VRAM e guardrails do compose prod.
- [x] Wave 1: Pacotes compartilhados (`engine-kit`, `heph-contracts`, codegen `openapi-typescript`).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
