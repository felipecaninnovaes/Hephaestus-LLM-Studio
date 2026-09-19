# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/wave-1-pacotes-compartilhados`
- **Fatia em andamento:** Wave 1 — Pacotes e Bibliotecas Compartilhadas (`tasks/consolidacao-auditoria-roadmap.md`).

## Checklist Imediato da Sessão Ativa (Wave 1)

- [x] RD-011: Centralização de `MOCK_MAGIC`, precisão de `mock_vector` e testes do `engine-kit`.
- [x] RD-012: Pipeline de geração de tipos na Web via `openapi-typescript`.
- [x] RD-010: Criação da crate compartilhada `heph-contracts` no workspace Rust.
- [x] Verificação e testes da Wave 1.
## Entregas Concluídas Recentemente
- [x] Modularização completa do Web Studio (`tasks/web-modularizacao-auditoria.md` Fases 1 a 6).
- [x] Modularização do Orchestrator em 8 fatias (`tasks/specs/orchestrator-modularization.md`).
- [x] Documentação Canônica de Infraestrutura (`docs/infra/` overview, storage, gpu-nodes).
- [x] Consolidação transversal e roadmap unificado (`tasks/consolidacao-auditoria-roadmap.md`).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
