# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `refactor/modularizacao-engines` (aberta de `develop`)
- **Fatia em andamento:** Otimização e Modularização das Engines (Redução de Tamanho de Arquivos & Desacoplamento Arquitetural).
  Foco: Desacoplar god procedures (`flux.py` 1400L, `runner.py` 719L, `sdxl.py` 799L, `sd15.py` 734L, `qwen_image.py` 635L) em loaders e estratégias dedicadas, elevar VRAM context ao `engine-kit` e otimizar `.dockerignore`.
  Spec: `tasks/specs/engines-modularizacao.md`.
- **HOTFIX permissões nó GPU (2026-09-22):** engines uid 1000 não escreviam em
  dir de job root:0755 (EACCES pós-geração). Bridge NO NÓ: `ENGINE_USER: "0:0"`
  em `infra/compose.gpu.yaml` (+`.bak-perms`). Fix permanente na branch
  `fix/permissoes-volume-engine-uid` (create_dir_all_open 0777 + probe engine).
  **Ao deployar o fix: remover ENGINE_USER do compose do nó e reiniciar
  orchestrator-gpu; depois `docker exec gpu-orchestrator-gpu-1 find /data/outputs /data/datasets -type d -exec chmod a+rwX {} +`**
  (dirs criados root durante a bridge).
- **Pendência do provider (Quitada 2026-09-23):** subagentes migrados do provedor
  opencode/muse-spark descontinuado para `google-antigravity/gemini-3.8-flash:low`
  no role `worker` em `.omp/config.yml`. Arquitetura multi-agente reconfigurada com
  novo subagente dedicado `@docs`.

## Checklist Imediato da Sessão Ativa
- [x] MCP RunPod em `.omp/mcp.json` (hosted OAuth + docs server)
- [x] `infra/Dockerfile.runpod-worker` + entrypoint DinD (dockerd interno, rede `heph-engine`, nvidia runtime)
- [x] Smoke test local do pod privilegiado (`/health` ok, runtime nvidia, rede criada)
- [x] Runbook `docs/infra/runpod-worker.md` (template via MCP/REST/Console + conectividade)
- [ ] Validar com conta RunPod real (tier privileged, pod de teste, adoção via UI)
## Entregas Concluídas Recentemente
- [x] Hotfix treino Qwen-Image-2.1: corrigido shadowing da variável `alpha` (LoRA) por tensor do canal alpha da imagem (`torch.ones((1,1,1,H,W))`) que causava `RuntimeError: The size of tensor a (4096) must match the size of tensor b (1024) at non-singleton dimension 4` no forward pass do LoRA; corrigida checagem de `image_pad_mask` em `_generate_sample_qwen` evitando `TypeError` no `QwenImage21Pipeline`; corrigido vazamento de VRAM do Text Encoder onde `pipe_kwargs["text_encoder"]` e referências internas em `text_pipeline.components` mantinham 6.3 GB presos na GPU (agora caindo para 0.01 GB); cobertura de resolução via `lora_cfg.resolution` e autocast bfloat16 adicionados. Validado com 227 testes em `trainer-difusao` e imagem `:gpu` reconstruída com `--no-cache` e smoke test de treino e amostra 100% aprovado no nó TrueNAS.
- [x] Modularização e Otimização das Engines (`refactor/modularizacao-engines`): criação de `trainer_difusao/loaders/` (quant_cache, transformer_loader, text_encoder_loader), `trainer_difusao/models/sd_pkg/` (embeddings, sample), helpers atômicos em `lora_io`, context manager de VRAM em `engine-kit`, `.dockerignore` dedicado nas engines, spec `tasks/specs/engines-modularizacao.md`. 363 testes passando em todas as engines; auditado e aprovado pelo `@reviewer`.
- [x] Suporte transversal ao Qwen-Image-2.1 (`packages/`, `engines/trainer-difusao`, `services/`, `apps/web`).
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).
- [x] Hotfix daemon difusão exit 125 no nó GPU: `DIFFUSION_TRAINER_IMAGE` propagado aos dois composes + `env.gpu.example`; tag `:local→:gpu` aplicada direto no TrueNAS (contorna até deploy); smoke `/health` 200 via DNS `diffusion-daemon:8766` dentro do `orchestrator-gpu`. Lição promovida a PITFALLS (2ª recorrência).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
