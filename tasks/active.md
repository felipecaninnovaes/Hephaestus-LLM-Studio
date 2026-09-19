# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `develop`
- **Fatia em andamento:** Nenhuma no momento — Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.

## Checklist Imediato da Sessão Ativa
- [x] Fase 1: Ingress Caddy TLS/headers/metrics, compose.gpu.yaml dinâmico, fail-fast prod (commit c02ac1e)
- [x] Fase 2: Binds dev 127.0.0.1, fechamento portas prod, rotação de logs (commit 3b143f0)
- [x] Fase 3: Healthchecks, resolver race condition principal/manager, s3-init sem apk dinâmico (commit c9d8605)
- [x] Fase 4: Volume models GPU, tuning postgres, parametrização heartbeat, backup/restore unificado (commit 69e84fb)
- [x] Fase 5: Standalone web, shellcheck scripts, limites de recursos prod (commit 3d5193f)
- [x] Verificação final em todas as combinações compose (dev, gpu, integ, prod e fail-fast)
## Entregas Concluídas Recentemente
- [x] Wave 0: Fundação de contratos OpenAPI, normalização de políticas VRAM e guardrails do compose prod.
- [x] Wave 1: Pacotes compartilhados (`engine-kit`, `heph-contracts`, codegen `openapi-typescript`).
- [x] Wave 2: Execução e orquestração (`control_package_ref`, abort semântico, `telemetry.jsonl`, non-root).
- [x] Wave 3: Borda e serviços (`flush_interval -1`, cookie HTTPS dinâmico, retries no manager client).
- [x] Wave 4: Frontend e interface (tipos estritos camelCase, decomposição de jobs/page e gallery).
- [x] Wave 5: Observabilidade, E2E e limpeza final (paridade golden, sync docs, encerramento do roadmap).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
