# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/onboarding-ux-proxy-e-worker-join`
- **Fatia em andamento:** Documentação e Instalador One-Line — Script `scripts/install.sh` (instalação via curl sem clone do repo) e `README.md` raiz com guia completo de setup e arquitetura.

## Checklist Imediato da Sessão Ativa
- [x] Criar `scripts/install.sh` para download e setup direto via `curl | bash`
- [x] Criar `README.md` canônico na raiz documentando os 4 modelos e o one-line quickstart
- [x] Testar instalação via `scripts/install.sh` na VM Ubuntu
- [x] Validar sintaxe dos scripts (`bash -n`) e commit convencional
## Entregas Concluídas Recentemente
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
