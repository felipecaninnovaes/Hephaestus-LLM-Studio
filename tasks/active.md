# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `refactor/trainer-difusao-unificacao-fase-c`
- **Fatia em andamento:** Unificação dos 4 trainers de difusão via Template Method — Fase C (reduzida): migração de `prompt_cache` (RAM) para `TextEmbedsCache` (disco) em `qwen_image.py` (`refactor/trainer-difusao-unificacao-fase-c`). Spec: `tasks/specs/trainer-difusao-unificacao-modelos.md`.
- **Última fatia integrada:** Fase B da unificação de trainers — `flux.py` (Flux.1+Flux.2-Klein) via `TrainingLoopRunner` (`refactor/trainer-difusao-unificacao-fase-b` mergeada em `develop` commit `f94541c`, smoke real GPU aprovado com FLUX.2-Klein-4B real).
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


## Checklist Imediato — Fase A (Unificação sd15/sdxl)
- [x] Spec aprovada: `tasks/specs/trainer-difusao-unificacao-modelos.md` (evidência: 71% similaridade textual sd15×sdxl medida via `difflib`)
- [x] `models/loop.py`: `LoraTrainConfig`, `parse_lora_train_config`, `ModelAdapter` (Protocol), `TrainingLoopRunner`
- [x] `models/sd_family/adapter.py`: `SD15Adapter`, `SDXLAdapter`
- [x] `sd15.py` (644→28L) e `sdxl.py` (666→32L) reduzidos a wrappers finos
- [x] Suíte `uv run pytest` verde: 229 passed (3 falhas pré-existentes em `test_qwen_image.py`, fora de escopo, confirmadas idênticas em `develop` via `git stash`)
- [x] Auditoria `@reviewer`: 1ª rodada REPROVA (duplicação de `emit_metric(setup_lora)`, `Protocol` incompleto, mensagens de erro genéricas) → 3 fixes aplicados → 2ª rodada APROVA sem ressalvas
- [x] Commit `92fe327` na branch `refactor/trainer-difusao-unificacao-fase-a`
- [x] **Smoke real GPU concluído (2026-09-26):** nó `dockeruser@10.15.1.2` livre (RTX 3060 182 MiB em uso). Build da imagem `hephaestus/trainer-difusao:smoke-fase-a` (cache Docker reaproveitado, só o `COPY` do código mudou). Dataset sintético (4 imagens 512×512 + captions). 1 época real (`ENGINE_MOCK=0`, pesos HF reais baixados on-the-fly) para **SD15** e **SDXL**: ambos `Exited (0)`, `metrics.jsonl` com as 10 fases esperadas (`init→loading_models→setup_lora→dataset_ready→generating_baseline_sample→baseline_ready→training_started→training→epoch_complete→completed`), checkpoint por época + adapter final salvos como safetensors válidos (1120 tensores LoRA no SDXL, header `"epoch":"1"` no checkpoint intermediário e ausente no final — confirma o fix pós-review), amostras baseline/época geradas, VRAM liberada ao final (3 MiB residual). Nó limpo pós-smoke.
- [x] Merge `refactor/trainer-difusao-unificacao-fase-a` → `develop` (commit `d062c93`)
- [x] **Fix colateral:** 3 testes desatualizados em `test_qwen_image.py` corrigidos (causa raiz: 2 commits legítimos e intencionais do próprio autor pré-sessão — `091b46c`/`a570f66`/`71ae71b` — mudaram comportamento sem atualizar os testes; NÃO era regressão da Fase A, `qwen_image.py` nunca tocado). 232 testes verdes (commit `754cb2d`).
- [x] Campo `extra: dict[str, Any] = field(default_factory=dict)` adicionado em `LoraTrainConfig` (aditivo, commit `bea3efd`) para acomodar estado específico de arquitetura não generalizável (hf_token, quant_label, is_flux2 no Flux).

## Checklist Imediato — Fase B (Unificação flux.py: Flux.1 + Flux.2-Klein)
- [x] `models/flux_adapter.py` (798L): `FluxAdapter` implementando `ModelAdapter`, cobrindo as 2 famílias via branch interno `is_flux2` (packing de latents, normalização VAE, flow-matching, quantização 4/8/2/6-bit com cache em disco e callbacks de telemetria)
- [x] `flux.py` (1026→54L) reduzido a wrapper fino
- [x] **2 bugs críticos encontrados e corrigidos pelo orchestrator ANTES do gate:** (1) `forward_and_loss` chamava `_cached_encode(..., cached_encode.get("text_cache"), ...)` — `cached_encode` já é o dict resolvido, sem chave `"text_cache"` → `AttributeError: 'NoneType' object has no attribute 'enabled'` no 1º batch real; corrigido para consumir `cached_encode["hidden"]`/`["pooled"]` direto (mesmo padrão SD15/SDXL). (2) `checkpoint_metadata` lia `tcfg.quantization` (helper genérico, ignora env `FLUX_QUANTIZATION`) em vez da resolução local correta → metadata podia mentir sobre a quantização real; corrigido para `tcfg.extra["quantization"]`.
- [x] Suíte `uv run pytest` verde: 232 passed, zero regressão
- [x] Auditoria `@reviewer`: APROVA sem ressalvas (8 critérios, confirmou os 2 fixes aplicados)
- [x] Commit `037d2c3` na branch `refactor/trainer-difusao-unificacao-fase-b`
- [x] **Smoke real GPU concluído (2026-09-26):** FLUX.2-Klein-4B real (`black-forest-labs/FLUX.2-klein-base-4B`, público, sem HF_TOKEN necessário), quantização 4-bit NF4 real (transformer 1.9GB + text encoder Qwen3 persistidos em cache), 1 época real, `Exited (0)`, `metrics.jsonl` com as fases esperadas incluindo os callbacks de quantização (`quantizing_transformer→transformer_ready→quantizing_text_encoder→text_encoder_ready`), 3.440.640 parâmetros LoRA treináveis / 1.937.776.128 congelados, checkpoint metadata confirma AMBOS os fixes (`"quantization":"4bit"`, `"base_model":"flux-2-klein-4b"`, `"epoch":"1"` só no intermediário). Nó limpo pós-smoke.
- [x] Merge `refactor/trainer-difusao-unificacao-fase-b` → `develop` (commit `f94541c`)
- [x] **Decisão de escopo pós-Fase-B (usuário, via `ask`):** Fase C completa (Template Method em `qwen_image.py`) avaliada e DESCARTADA — modelo flagship em produção, física própria domina as 920 linhas (VLM precompute em duas fases, latents pré-computados por índice, kwargs dinâmicos por introspecção de assinatura), ganho estrutural pequeno vs risco alto (2 bugs críticos já encontrados na Fase B, mais simples). Escopo reduzido ao item #12 de `engines-auditoria-global.md`.

## Checklist Imediato — Fase C reduzida (prompt_cache → TextEmbedsCache em qwen_image.py)
- [x] Único arquivo tocado: `models/qwen_image.py`. `prompt_cache: dict[str, tuple]` em RAM → `TextEmbedsCache` em disco (mesmo padrão já usado por sd15/sdxl/flux). Precompute loop (retry adaptativo de OOM, VLM 4-bit com fallback CPU, unload pós-precompute) preservado intocado — só o backend de armazenamento mudou. Payload nunca grava `None` (evita incompatibilidade com `torch.load(weights_only=True)`). Fallback de dummy-embed preservado.
- [x] Suíte `uv run pytest` verde: 232 passed, zero regressão (confirmado 2x localmente)
- [x] Auditoria `@reviewer`: APROVA COM RESSALVAS (ressalva é só falta de shell no ambiente do reviewer p/ rodar pytest — zero blocking findings nos 5 critérios)
- [x] Commit `03e43ab` na branch `refactor/trainer-difusao-unificacao-fase-c`
- [x] **Smoke real GPU concluído (2026-09-26):** Qwen-Image-2.1 real (7B DiT + VLM Qwen3 4-bit), 1 época real, `Exited (0)`. Loss variando naturalmente por step (0.336→0.263→0.185→0.397, EMA 0.32) — confirma que `text_cache.get()` retornou embeddings reais cacheados, não caiu no fallback dummy-zero. `text_embeds_cache/{hash}.pt` (116KB) criado em disco confirmando a migração RAM→disco funcionando end-to-end. Checkpoint safetensors válido (256 tensores LoRA, 4.194.304 treináveis / 3.561.762.816 congelados). Nó limpo pós-smoke.
- [ ] Merge `refactor/trainer-difusao-unificacao-fase-c` → `develop`


## Checklist Concluído — Telemetria ETA/VRAM (fatia anterior, integrada)
- [x] Definir contrato de campos preditivos (`etaSeconds`, `etaFormatted`, `stepTimeSeconds`, `vramReservedGb`) em `engine-kit` e `trainer-difusao`
- [x] Implementar cálculo de ETA móvel (EMA), medição de tempo por step e telemetria enriquecida em `engine_kit/telemetry.py` e `common_pkg/metrics.py`
- [x] Atualizar logs de terminal em `qwen_image.py`: micro-steps visíveis, intervalo adaptativo por tempo (emissão a cada passo para passos > 5s), barra de progresso e VRAM alocada/reservada
- [x] Integrar campos de ETA e VRAM reservada na interface Web (`apps/web` - Drawer de Jobs / Telemetria)
- [x] Validar testes unitários em `engine-kit`, `trainer-difusao` e testes do frontend
- [x] Auditoria com `@reviewer` (Gate Obrigatório)
- [x] Sincronização de documentação com `@docs`

## Entregas Concluídas Recentemente
- [x] Telemetria ao Vivo com ETA Preditivo, Métricas de VRAM e Logs Vivos de Treino (`feat/engines-live-telemetry-eta`):
  - **Contrato Canônico de Telemetria Preditiva:** Expansão de `TelemetryEmitter` em `engine-kit` e `MetricsLogger` em `trainer-difusao/common_pkg/metrics.py` com campos padronizados: `vramReservedGb`, `stepTimeSeconds`, `speed`, `etaSeconds` e `etaFormatted`.
  - **Cálculo Robusto de ETA (EMA):** Estimativa móvel exponencial (`alpha = 0.2`) de tempo por iteração protegida contra divisão por zero, amortecendo flutuações e viabilizando predição precisa de tempo restante em treinamentos longos.
  - **Terminal e Logs Adaptativos no Qwen-Image:** Intervalo adaptativo de emissão de telemetria e logs (passos > 5s emitem a cada passo em vez de esperar 5 passos fixos), com barra de progresso visual, exibição explícita de micro-steps/épocas e telemetria em tempo real de VRAM alocada e reservada.
  - **Interface Web Reativa e Acessível (`apps/web`):** Hook `useJobTelemetry` e componente `JobDrawer` integrados com suporte a visualização de ETA, velocidade de iteração, VRAM alocada/reservada e logs ao vivo com auto-scroll pausável ao rolar para cima.
  - **Testes e Qualidade:** 261 testes unitários em Python (`engine-kit` e `trainer-difusao`) e 30 testes unitários no frontend (`apps/web`) íntegros e passando.
  - **Auditoria:** Auditado e aprovado com veredito APROVA pelo `@reviewer`.
- [x] Mitigação de Vazamentos de Memória e VRAM/RAM no Qwen-Image-2.1 (`fix/qwen-image-memory-leaks`):
  - **Purga Incondicional de LoRA Residual no Daemon:** Invocação de `unload_lora_weights` incondicionalmente no pipeline em cache antes de avaliar e carregar novos adaptadores (`runner.py`), evitando poluição de inferências puras subsequentes e vazamento cumulativo de VRAM.
  - **Desacoplamento de Referências no Pipeline de Amostragem:** Esvaziamento de dicionários locais (`pipe_kwargs.clear()`), anulação explícita dos componentes (`pipe.components[k] = None`, `pipe.vae = None`, `pipe.transformer = None`, `del pipe`) e dupla liberação com `release_memory()` e `malloc_trim(0)` em bloco `finally` (`sample.py`).
  - **Desalocação Atômica Pós-Backward:** Destruição explícita de tensores intermediários (`pred`, `pred_img`, `packed_target`, `target`, `loss`, `trans_kwargs`, `batch_embeds`, etc.) logo após `loss.backward()` (`qwen_image.py`), prevenindo acúmulo de tensores antes da alocação de momentum/variância pelo AdamW.
  - **Contenção de Fragmentação PyTorch:** Invocação periódica de `cleanup_cuda()` entre épocas de treino e anulação de batches de treino para desalocação no driver CUDA.
  - **Testes Comportamentais:** Testes unitários em `tests/test_qwen_image.py` cobrindo ciclos de descarte de componentes, descarregamento de LoRA residual em inferência pura vs LoRA ativo, e limpeza de memória.
  - **Auditoria:** Auditado e aprovado com veredito APROVA pelo `@reviewer`.
- [x] Hardening de Infraestrutura & Segmentação de Redes Docker (`feat/infra-redes-segmentadas`):
  - **Segmentação em 3 Redes:** Fim da rede flat através da criação de `frontend_net`, `backend_net` e `engine_net` com escopos estritos.
  - **Isolamento Estrito de `web`:** Next.js isolado na `frontend_net`, sem acesso de rede ao banco de dados (`db`) nem ao S3 (`seaweedfs`).
  - **Roteamento de Ingress em Produção:** Serviço `ingress` (Caddy) restrito exclusivamente à `frontend_net` (`ports: 80/443`).
  - **Ponte Segura para Workloads:** `seaweedfs` e `orchestrator-local` configurados como pontes seguras (*dual-homed*) em `backend_net` e `engine_net`, viabilizando I/O de artefatos de treino sem expor banco ou frontend.
  - **Engines e Daemons:** `ENGINE_NETWORK` e `DIFFUSION_DAEMON_NETWORK` padronizados para `${COMPOSE_PROJECT_NAME:-infra}_engine_net`.
  - **Validação de Sintaxe e Testes:** 4 perfis compose validados sintaticamente (`dev`, `prod`, `integ`, `gpu`), 786 testes verdes no workspace Rust e suíte de testes de interface Next.js.
  - **Auditoria:** Auditado e aprovado pelo `@reviewer`.
- [x] Modularização e Acessibilidade Frontend (`feat/web-ui-modularizacao-a11y`):
  - **Acessibilidade WCAG 2.5.8:** Target size mínimo de 32x32px (`min-h-[32px] min-w-[32px]`) em botões de opção do `SegmentedControl`.
  - **Polling inteligente de telemetria em `Sidebar.tsx`:** Listener de `visibilitychange` interrompendo `setInterval` quando em abas ocultas e retomando com fetch imediato ao voltar à aba visível.
  - **Unificação de treino em `lib/datasets.ts`:** `canTrainDataset`, `trainDatasetDisabledReason` e `trainDatasetActionLabel` suportando datasets YOLO (modal in-place) e Difusão (redirecionamento para `/difusao?datasetId=`).
  - **Desacoplamento e tipagem de `Select.tsx`:** Hooks reutilizáveis `useFloatingPosition.ts` (posicionamento portaled com flip e listeners) e `useListboxNavigation.ts` (navegação WAI-ARIA por teclado), com eliminação total de `SelectProps<any>` por generic `<T extends string | number>`.
  - **Qualidade e testes:** 16 testes unitários no frontend passando, build Next.js 100% verde (14 rotas) e prova visual headless.
  - **Auditoria:** Auditado e aprovado pelo `@reviewer`.
- [x] Consumo Canônico de `heph-contracts` & Modularização de `jobs/` (`feat/api-principal-contracts-modularizacao`):
  - **Centralização de DTOs e Tipos Canônicos:** Centralização de DTOs e tipos de protocolo interno em `crates/heph-contracts` (`job_status.rs`, `jobs.rs`, `nodes.rs`, `models.rs`).
  - **Migração do Manager Client:** Migração de `services/api-principal/src/jobs/manager_client.rs` para consumir `heph-contracts`, eliminando ~260 linhas de DTOs `Internal*` duplicados manualmente.
  - **Decomposição Modular de Handlers:** Decomposição do monólito `services/api-principal/src/jobs/handlers.rs` (5.800 linhas) na pasta modular `services/api-principal/src/jobs/handlers/` (`mod.rs`, `types.rs`, `helpers.rs`, `query.rs`, `stream.rs`, `artifacts.rs`, `submit.rs`, `lifecycle.rs`, `apply.rs`, `tests.rs`), preservando 100% do wire OpenAPI `camelCase` e das rotas.
  - **Auditoria e Cobertura:** Aprovado pelo `@reviewer`, 786 testes unitários/contrato passando no workspace Rust (10 novos testes de contrato).
- [x] Autonomia e Resiliência do Orchestrator (`feat/orchestrator-autonomia`):
  - **P0-3 (Admissão atômica no dispatch):** transição para `state.try_admit` sob Mutex eliminando janela de concorrência TOCTOU e rejeitando duplicidade de `job_id` com HTTP 409 Conflict.
  - **P0-2 (Spool Outbox durável em disco):** persistência atômica (write temp + rename) em `$ORCH_WORKDIR/.outbox/` com drain periódico em background (5s) e flush no shutdown, garantindo entrega at-least-once de relatórios de conclusão/erro mesmo com o manager temporariamente fora do ar.
  - **P2-1 (Heartbeat adaptativo com backoff/jitter):** intervalo base de 2s escalando exponencialmente até 30s (+jitter determinístico) após falhas consecutivas de rede/manager, prevenindo tempestades de reconexão.
  - **P2-2 (Reaper periódico de containers órfãos em runtime):** loop a cada 60s reconciliando containers Docker `trainer-*` ativos com `active_jobs` em memória; tolerância de 300s de idade para evitar matar containers recém-spawnados; parada graciosa (`docker stop --time 5`) antes de `docker rm --force`.
  - **P2-5 (Graceful shutdown):** interceptação coordenada de SIGTERM/SIGINT no Axum com `with_graceful_shutdown`; encerramento ordenado cancelando background loops, aguardando jobs ativos por até 10s, interrompendo containers residuais, desativando o daemon de difusão HTTP e drenando a outbox em disco antes da saída.
  - Auditado e aprovado pelo `@reviewer` (124 testes unitários/integração passando sem regressões).
- [x] Hotfix treino Qwen-Image-2.1: corrigido shadowing da variável `alpha` (LoRA) por tensor do canal alpha da imagem (`torch.ones((1,1,1,H,W))`) que causava `RuntimeError: The size of tensor a (4096) must match the size of tensor b (1024) at non-singleton dimension 4` no forward pass do LoRA; corrigida checagem de `image_pad_mask` em `_generate_sample_qwen` evitando `TypeError` no `QwenImage21Pipeline`; corrigido vazamento de VRAM do Text Encoder onde `pipe_kwargs["text_encoder"]` e referências internas em `text_pipeline.components` mantinham 6.3 GB presos na GPU (agora caindo para 0.01 GB); cobertura de resolução via `lora_cfg.resolution` e autocast bfloat16 adicionados. Validado com 227 testes em `trainer-difusao` e imagem `:gpu` reconstruída com `--no-cache` e smoke test de treino e amostra 100% aprovado no nó TrueNAS.
- [x] Modularização e Otimização das Engines (`refactor/modularizacao-engines`): criação de `trainer_difusao/loaders/` (quant_cache, transformer_loader, text_encoder_loader), `trainer_difusao/models/sd_pkg/` (embeddings, sample), helpers atômicos em `lora_io`, context manager de VRAM em `engine-kit`, `.dockerignore` dedicado nas engines, spec arquivada em `docs/archive/specs/engines-modularizacao.md`. 363 testes passando em todas as engines; auditado e aprovado pelo `@reviewer`.
- [x] Suporte transversal ao Qwen-Image-2.1 (`packages/`, `engines/trainer-difusao`, `services/`, `apps/web`).
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).
- [x] Hotfix daemon difusão exit 125 no nó GPU: `DIFFUSION_TRAINER_IMAGE` propagado aos dois composes + `env.gpu.example`; tag `:local→:gpu` aplicada direto no TrueNAS (contorna até deploy); smoke `/health` 200 via DNS `diffusion-daemon:8766` dentro do `orchestrator-gpu`. Lição promovida a PITFALLS (2ª recorrência).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
