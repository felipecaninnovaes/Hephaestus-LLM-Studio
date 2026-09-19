# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/wave-4-frontend-web`
- **Fatia em andamento:** Wave 4 — Frontend e Interface do Usuário (`tasks/consolidacao-auditoria-roadmap.md`).

## Checklist Imediato da Sessão Ativa (Wave 4)

- [x] RD-040: Limpeza de duplicidades snake_case em `DiffusionJobParams` e nulabilidade de `datasetId`.
- [x] RD-040: Verificação de envio estrito em camelCase nas forjas e ActionCenter.
- [x] RD-041: Modularização de `app/(studio)/jobs/page.tsx` em subcomponentes.
- [x] RD-041: Modularização de `components/studio/GenerationGallery.tsx`.
- [x] Verificação e testes da Wave 4.
## Entregas Concluídas Recentemente
- [x] Wave 0: Fundação de contratos OpenAPI, normalização de políticas VRAM e guardrails do compose prod.
- [x] Wave 1: Pacotes compartilhados (`engine-kit`, `heph-contracts`, codegen `openapi-typescript`).
- [x] Wave 2: Execução e orquestração (`control_package_ref`, abort semântico, `telemetry.jsonl`, non-root).
- [x] Wave 3: Borda e serviços (`flush_interval -1`, cookie HTTPS dinâmico, retries no manager client).
## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
