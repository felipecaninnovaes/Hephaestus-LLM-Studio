# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `feat/public-release-compose-ghcr`
- **Fatia em andamento:** Preparação para Release Público — Workflows GitHub Actions (CI & GHCR Release), pasta `compose/` com os 4 modelos autônomos e script `setup.sh`.

## Checklist Imediato da Sessão Ativa
- [x] Criação de `.github/workflows/ci.yml` (matriz Rust, Web, Python, Compose)
- [x] Criação de `.github/workflows/release.yml` para publicação no GHCR (`ghcr.io/felipecaninnovaes/hephaestus-*`)
- [x] Criação da pasta `compose/` com os 4 modelos canônicos:
  - [x] `compose/local-com-local-node.yaml` (CPU/Mock)
  - [x] `compose/local-com-local-node-gpu.yaml` (NVIDIA GPU local)
  - [x] `compose/local-sem-node.yaml` (Control Plane para nós remotos)
  - [x] `compose/remote-node.yaml` (Worker GPU remoto / TrueNAS)
- [x] Criação de arquivos de apoio: `compose/.env.example`, `compose/Caddyfile`, `compose/seaweedfs-s3.json`, `compose/ensure-bucket.sh`, `compose/README.md`
- [x] Criação de `scripts/setup.sh` (assistente interativo + flag `--auto` para geração segura de credenciais)
- [x] Validação sintática de todos os arquivos compose via `docker compose config`

## Entregas Concluídas Recentemente
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
