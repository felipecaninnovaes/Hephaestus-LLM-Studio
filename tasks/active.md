# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `chore/daemon-imagem-gpu` (aberta de `develop`; push/merge aguardam ordem)
- **Fatia em andamento:** Observabilidade & Reprodutibilidade do Treino —
  **código 100% commitado e verde** (C1/C2a/C2b/C2c/F1/F2; spec
  `tasks/specs/treino-observabilidade.md`). Falta apenas o **deploy na janela
  segura** (fim do treino atual): rebuild de imagens + restart de orchestrator,
  api-principal e trainer-difusao. Web já está viva no dev server.
- **HOTFIX permissões nó GPU (2026-09-22):** engines uid 1000 não escreviam em
  dir de job root:0755 (EACCES pós-geração). Bridge NO NÓ: `ENGINE_USER: "0:0"`
  em `infra/compose.gpu.yaml` (+`.bak-perms`). Fix permanente na branch
  `fix/permissoes-volume-engine-uid` (create_dir_all_open 0777 + probe engine).
  **Ao deployar o fix: remover ENGINE_USER do compose do nó e reiniciar
  orchestrator-gpu; depois `docker exec gpu-orchestrator-gpu-1 find /data/outputs /data/datasets -type d -exec chmod a+rwX {} +`**
  (dirs criados root durante a bridge).
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
- [x] Hotfix daemon difusão exit 125 no nó GPU: `DIFFUSION_TRAINER_IMAGE` propagado aos dois composes + `env.gpu.example`; tag `:local→:gpu` aplicada direto no TrueNAS (contorna até deploy); smoke `/health` 200 via DNS `diffusion-daemon:8766` dentro do `orchestrator-gpu`. Lição promovida a PITFALLS (2ª recorrência).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
