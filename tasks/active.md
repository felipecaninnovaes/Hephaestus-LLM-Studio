# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/onboarding-ux-proxy-e-worker-join`
- **Fatia em andamento:** Suporte a worker na RunPod — MCP oficial configurado no harness, imagem DinD `hephaestus-runpod-worker` e runbook de template.

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
