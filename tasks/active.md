# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `fix/treino-observabilidade`
- **Fatia em andamento:** Observabilidade & Reprodutibilidade do Treino —
  **código 100% commitado e verde** (C1/C2a/C2b/C2c/F1/F2; spec
  `tasks/specs/treino-observabilidade.md`). Falta apenas o **deploy na janela
  segura** (fim do treino atual): rebuild de imagens + restart de orchestrator,
  api-principal e trainer-difusao. Web já está viva no dev server.
- **Pendência do provider:** subagentes (`opencode-go/muse-spark`) sem fundos
  desde 2026-09-20 (402) — Wave 2 executada inline pelo coordenador. Recarregar
  ou repontar os roles em `.omp/` antes da próxima delegação.

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
