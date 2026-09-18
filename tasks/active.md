# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `chore/reestruturacao-docs`
- **Fatia em andamento:** Reestruturação da documentação e memória do monorepo (eliminação de monolitos >150 linhas, separação estrita entre `docs/` e `tasks/`).

---

## Checklist Imediato da Sessão Ativa

- [x] Criação de `docs/archive/specs/` e migração do spec concluído de modularização
- [x] Criação de `tasks/specs/` e migração dos specs ativos (`qlora-difusao`, `infra-autonomia`, `backend-autonomia`)
- [x] Consolidação de pendências e dívidas em `tasks/backlog.md` (< 120 linhas)
- [x] Criação de `tasks/active.md` (< 70 linhas) substituindo `tasks/todo.md`
- [x] Remoção de arquivos obsoletos (`tasks/todo.md`, `docs/dividas.md`, `tasks/impruvements/`)
- [x] Conclusão dos agentes pares (`ServicesDocsWriter`, `EnginesWebDocsWriter`)
- [x] Validação global do coordenador (contagem de linhas, integridade de referências)

---

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
