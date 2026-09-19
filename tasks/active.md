# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/wave-3-borda-servicos-aplicacao`
- **Fatia em andamento:** Wave 3 — Borda e Serviços de Aplicação (`tasks/consolidacao-auditoria-roadmap.md`).

## Checklist Imediato da Sessão Ativa (Wave 3)

- [x] RD-030: Otimização de SSE com `flush_interval -1` no Caddyfile.
- [x] RD-031: Detecção dinâmica de HTTPS (`X-Forwarded-Proto`) para cookie `heph_session` `Secure`.
- [x] RD-032: Sweeper periódico de `job_prepares` e retries com backoff no cliente do manager.
- [x] Verificação e testes da Wave 3.
## Entregas Concluídas Recentemente
- [x] Wave 0: Fundação de contratos OpenAPI, normalização de políticas VRAM e guardrails do compose prod.
- [x] Wave 1: Pacotes compartilhados (`engine-kit`, `heph-contracts`, codegen `openapi-typescript`).
- [x] Wave 2: Execução e orquestração (`control_package_ref`, abort semântico, `telemetry.jsonl`, non-root).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
