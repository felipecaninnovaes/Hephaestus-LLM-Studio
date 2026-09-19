# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/onboarding-ux-proxy-e-worker-join`
- **Fatia em andamento:** Melhoria de Onboarding e Pareamento — Modo Proxy de Storage por Padrão, Auto-detecção de GPU no `setup.sh`, Exportador de Configuração de Worker Remoto e Redirect de Rotas.

## Checklist Imediato da Sessão Ativa
- [x] Configurar modo proxy de storage por padrão nos templates compose (`S3_PUBLIC_ENDPOINT_URL=""`)
- [x] Aprimorar `scripts/setup.sh` com auto-detecção de GPU NVIDIA vs CPU-only e recomendação de perfis
- [x] Adicionar helper `export-worker-env` no `scripts/heph.sh` para facilitar pareamento remoto
- [x] Adicionar redirect de `/orchestrators` -> `/environments` em `apps/web/next.config.ts`
- [x] Validar sintaxe `docker compose config` dos templates e execução dos scripts
- [x] Validar build web (`npm run build --workspace=web`) e workspace Rust (`cargo check --workspace`)
## Entregas Concluídas Recentemente
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
