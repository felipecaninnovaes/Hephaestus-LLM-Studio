# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/wave-0-fundacao-contratos`
- **Fatia em andamento:** Wave 0 — Fundação, Contratos Canônicos e Guardrails (`tasks/consolidacao-auditoria-roadmap.md`).

## Checklist Imediato da Sessão Ativa (Wave 0)

- [x] RD-001: Padronização OpenAPI (`totalSteps`, `totalEpochs`, `JobParams` tipados, `orchestratorKind`, nulabilidade de `datasetId` em `Job`).
- [x] RD-002: Normalização de chaves em `vram-table.yaml` e `engines.yaml` (`engine: diffusion`, `flux-2-klein-4b`, modos auxiliares).
- [x] RD-003: Fail-fast de segredos e fechamento de portas no Compose Prod (`compose.prod.yaml`, remoção de `STUDIO_MASTER_KEY`).
- [x] Verificação e testes de contrato da Wave 0.
## Entregas Concluídas Recentemente
- [x] Modularização completa do Web Studio (`tasks/web-modularizacao-auditoria.md` Fases 1 a 6).
- [x] Modularização do Orchestrator em 8 fatias (`tasks/specs/orchestrator-modularization.md`).
- [x] Documentação Canônica de Infraestrutura (`docs/infra/` overview, storage, gpu-nodes).
- [x] Consolidação transversal e roadmap unificado (`tasks/consolidacao-auditoria-roadmap.md`).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
