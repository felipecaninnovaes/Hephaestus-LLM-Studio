# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `fix/telemetry-time-in-message-and-ui`
- **Fatia em andamento:** Propagação de Tempo/Velocidade e ETA na String de Mensagem e Extração Resiliente no Frontend (`fix/telemetry-time-in-message-and-ui`).
- **Última fatia integrada:** Telemetria ao Vivo com ETA Preditivo, Métricas de VRAM e Logs Vivos de Treino (`feat/engines-live-telemetry-eta` mergeada com sucesso em `develop`).
  Auditado e aprovado pelo `@reviewer`, 261 testes unitários em Python, 30 testes no frontend, cálculo de ETA preditivo por EMA, telemetria de VRAM alocada/reservada e logs de micro-steps/steps adaptativos.
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
