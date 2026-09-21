# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `fix/treino-observabilidade`
- **Fatia em andamento:** Observabilidade & Reprodutibilidade do Treino (spec
  `tasks/specs/treino-observabilidade.md`) — Wave 1: C2b/C1/C2c/F1; Wave 2: C2a/F2.
- **⚠ CONSTRAINT (2026-09-20):** treino em andamento — proibido `docker compose
  up/restart/down/build` e qualquer kill de portas dos serviços/engines. Só
  `apps/web` pode ser reiniciada. Deploy das correções de orquestrador/manager/BFF
  fica para janela segura; nesta fatia apenas código + testes.

## Checklist Imediato da Sessão Ativa
- [x] MCP RunPod em `.omp/mcp.json` (hosted OAuth + docs server)
- [x] `infra/Dockerfile.runpod-worker` + entrypoint DinD (dockerd interno, rede `heph-engine`, nvidia runtime)
- [x] Smoke test local do pod privilegiado (`/health` ok, runtime nvidia, rede criada)
- [x] Runbook `docs/infra/runpod-worker.md` (template via MCP/REST/Console + conectividade)
- [ ] Validar com conta RunPod real (tier privileged, pod de teste, adoção via UI)
## Entregas Concluídas Recentemente
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
