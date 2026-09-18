# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/qlora-difusao`
- **Fatia em andamento:** Implementação de QLoRA para Modelos de Difusão (FLUX, SDXL e SD 1.5) conforme `tasks/specs/qlora-difusao.md`.

---

## Checklist Imediato da Sessão Ativa

- [x] Fase 1: Motor de Treino Difusão (optimizers.py, sd15.py, sdxl.py, flux.py, mock.py, testes)
- [x] Fase 2: Contratos OpenAPI e Validação no api-principal Rust
- [x] Fase 3: Políticas de VRAM (vram-table.yaml)
- [x] Fase 4: Interface Web Studio (types/studio.ts, jobs.ts e ForjaDifusaoSetup.tsx)
- [x] Fase 5: Validação completa (Python pytest 160/160, Cargo test 708/708, Web build, Docker config)
---

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
