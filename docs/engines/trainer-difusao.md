# Trainer Difusão (`engines/trainer-difusao`)

O `trainer-difusao` é o motor de difusão do Hephaestus, responsável pelo treinamento de adapters LoRA/QLoRA e execução de inferência em modelos generativos de imagem. Suporta execução real acelerada por GPU (via PyTorch, Diffusers e PEFT) e execução simulada determinística (`ENGINE_MOCK=1`).

---

## Arquitetura Modular

O pacote foi desenhado para desacoplar modelos, pipelines de inferência e ciclo de vida do daemon:

### 1. `models/` e `models/flux_pkg/`
Implementações de treinamento organizadas sob a abstração `BaseModelTrainer`:
- **`FluxTrainer` / `flux_pkg/`**: Suporte ao FLUX.2 Klein 4B e 9B base (ver §5). Implementa `rope.py` (Rotary Positional Embeddings 3D), `quant_cache.py` (quantização de pesos e cache), `encoding.py` e rotinas especializadas de amostragem em `sample.py`.
- **`SDXLTrainer`**: Treinamento para Stable Diffusion XL 1.0 com duplo text-encoder (CLIP ViT-L e OpenCLIP ViT-bigG) e condicionamento por tamanho/crop.
- **`SD15Trainer`**: Treinamento para Stable Diffusion 1.5 clássico (UNet e CLIP Text Encoder).
- **`MockTrainer`**: Emite loss decrescente determinística e gera arquivos `.safetensors` simulados sem alocar GPU.

### 2. `common_pkg/`
Primitivas reutilizáveis entre todos os modelos:
- **`train_config.py`**: Parser e validação estrita do arquivo de configuração do job.
- **`lora_io.py`**: Serialização, formatação e salvamento de pesos LoRA em formato safetensors compatível com o ecossistema.
- **`text_embeds.py`**: Pré-computação de embeddings de texto em cache para aceleração do loop de treino.
- **`encoder_merge.py`**: Utilitários para injeção e fusão de encoders de texto.

### 3. `generation/`
Pipeline de inferência pontual e em lote:
- **`runner.py`**: Orquestra a execução da geração de imagens, seed determinística e pós-processamento.
- **`pipelines.py`**: Carregamento sob demanda e cache de pipelines `diffusers` (ex.: `FluxPipeline`, `StableDiffusionXLPipeline`).
- **`adapters.py`**: `DaemonLoraCache` — cache LRU de adapters LoRA nomeados no pipeline quente (`DIFFUSION_DAEMON_LORA_CACHE`, default 4; chave = path + size + mtime + md5 parcial). Reusa adapters via `set_adapters`; requisição sem LoRA chama `disable_lora`/`set_adapters([])` e a reativação chama `enable_lora` antes de `set_adapters`. No img2img (`sdxl`/`sd15`), o pipeline variante `_I2I(**pipe.components)` recebe os adapters de novo (não herda o estado). Trocar o pipeline base limpa o cache (`clear_daemon_lora_cache`). Ver `docs/PITFALLS.md` (LoRA residual).
  - **Layout da engine**: o adapter salvo por `_save_lora_safetensors` não tem prefixo de componente (contrato ComfyUI, não muda). `prepare_lora_for_load` remapeia esse layout para `transformer.` (Klein) ou `unet.` (sdxl/sd15) antes de `load_lora_weights` (senão o diffusers filtra por prefixo e carrega 0 chaves só com warning); arquivos já prefixados / kohya / `diffusion_model.` passam intactos.
  - **Zero chaves = erro**: arquivo sem tensor LoRA, ou adapter que não injeta nenhum módulo (`count_injected_lora_modules`), levanta `LoraLoadError` nomeando o arquivo — `generate` sai com erro, o daemon responde HTTP 500 `generation_failed`.
- **`text_encoder.py`**: Suporte para text encoders oficiais e text encoders customizados definidos no catálogo. `load_flux2_quantized_components` carrega transformer + Qwen3 do Klein quantizados (`4bit`/`8bit` bitsandbytes, `2bit`/`6bit` torchao) pelos mesmos loaders do treino e **no mesmo cache quantizado persistido** (`resolve_quant_base_dir(model_id, quant, custom_identity, text_encoder_path)`): hit reaproveita sem re-quantizar. `quantization: none` mantém bf16. Componente que não sai quantizado aborta (nunca bf16 silencioso). A chave do pipeline residente do daemon já inclui `quantization`.
- **`progress.py`**: Callback de progresso por timestep/amostragem reportado em tempo real ao `engine-kit.telemetry`.

### 4. `serve_pkg/` e `serve.py`
Daemon HTTP de longa duração para inferência interativa, gerenciado pelo orchestrator:
- **Endpoints**: `/health` (status, spec carregada, uso de VRAM), `/generate` (execução da geração) e `/shutdown` (encerramento gracioso).
- **`state.py`**: Mantém o modelo aquecido em memória (`loaded_spec`), gerencia trava de concorrência (`_busy`) para garantir inferência atômica e computa tempos de atividade.
- **Mounts**: o container do daemon monta os volumes em `/datasets` e `/outputs`, iguais ao one-shot; todo path do `config.yaml` (LoRAs, checkpoint/text encoder custom, init, control) e o `output_dir` (`/outputs/<job>`) chegam nesse namespace. Os caches HF/quantização do daemon ficam em `/outputs/.cache` (antes `/data/outputs/.cache`).
- **Input pedido inexistente**: `generation/config.py::check_requested_inputs` roda antes de qualquer carga de pipeline e levanta `MissingInputError` nomeando o path para `loras[i]`/`weights_path`, `custom_checkpoint_path`, `text_encoder_path` e `init_image_path` — no daemon vira HTTP 500 `generation_failed`, no one-shot `_die` (exit 1); nunca degrada em silêncio para o modelo base.

### 5. Família FLUX.2 Klein (`klein.py`) — 4B e 9B base
Registro único arch → (repo base, repo destilado, env): o **arch do config decide o repo**, então 4B e 9B coexistem no mesmo nó. `flux-2-klein-4b` (aliases legados `flux`/`flux2`/`flux-2`): `FLUX_MODEL_ID` / `FLUX_DISTILLED_MODEL_ID` ou `black-forest-labs/FLUX.2-klein-base-4B` / `FLUX.2-klein-4B`. `flux-2-klein-9b` (só o base, sem destilado): `FLUX_9B_MODEL_ID` ou `black-forest-labs/FLUX.2-klein-base-9B`. Nenhuma env de uma variante vaza para a outra. Usado pelo treino (`FluxAdapter`), por `generation/text_encoder.py::_flux2_repo_id` e pelos aliases de `_canonical_model_name`.
- **Treino 9B**: `model: flux-2-klein-9b` força `cache_text_embeddings` e o **unload do text encoder** após o pré-compute (`models/loop.py::resolve_text_encoder_unload`, `extra["force_text_encoder_unload"]`), ignorando `ENABLE_TEXT_ENCODER_UNLOAD` e resume — sem isso, transformer 9B + Qwen3 estouram 12 GB (spike RTX 3060: 768/1024 px, rank 16/32, pico ≈11,9 GiB no pré-compute; loop 7,4–9,0 GiB; ≈2× o tempo do 4B por step). Metadata do adapter: `base_model=flux-2-klein-9b` (4B: `flux-2-klein-4b`).
- **Geração 9B**: `base_model: flux-2-klein-9b` usa `Flux2KleinPipeline` do repo 9B com quantização real (`load_flux2_quantized_components`), mesma allowlist de samplers Flux.2 (`default|euler|heun`) e o mesmo remap de LoRA `transformer.`. A chave/spec do pipeline residente do daemon inclui `base_model`: trocar 4B↔9B recarrega o pipeline (o anterior é liberado e `empty_cache` roda antes do load; um pipeline por vez).
- **Rejeições explícitas (`_die`)**: no 9B, `custom_checkpoint_path`, `text_encoder_path` (treino e geração) e `distilled: true` (geração). Uma LoRA de outra variante falha ao carregar (shapes diferentes; `LoraLoadError`).
- **Env**: `FLUX_9B_MODEL_ID` (opcional; override do repo 9B) vive ao lado de `FLUX_MODEL_ID`/`FLUX_DISTILLED_MODEL_ID` nas envs do nó GPU.

---

## Treino LoRA e QLoRA

- **Adapters PEFT**: Injeta adaptadores de baixo posto (*Low-Rank Adaptation*) nas camadas lineares e de atenção dos modelos base.
- **Quantização BitsAndBytes (`quantization.py`)**: Suporte a quantização de 4-bit (`nf4`, `fp4`) e 8-bit (`int8`), permitindo o fine-tuning de modelos pesados em GPUs de consumo.
- **Configurações e Limites de VRAM**: Os limites operacionais de VRAM para cada modelo e tipo de quantização são canônicos em `packages/policies/vram-table.yaml`.
- **Resume, LR e hiperparâmetros (`TrainingLoopRunner`: flux, sd15, sdxl; loop próprio do `qwen_image` com a mesma semântica)**: em retomada (`epoch_offset > 0`) `lora.lr_resume_mode` (`continue` padrão | `restart`; inválido ⇒ erro) decide o LR de pico: `continue` usa o `lr` do optimizer state restaurado (sem state ⇒ `restart` + aviso), `restart` usa `learningRate`; a curva nova cobre só `steps_per_epoch * epochs`, do passo 0, com o warmup da requisição (`_build_optimizer_and_scheduler` em `optimizers.py`, compartilhada por Qwen e loop). Treino novo ignora o modo. `steps_per_epoch` = passos de otimizador = `ceil(N/gradient_accumulation_steps)`; `global_step`, progresso, ETA e `lr` nas métricas contam passos de otimizador. `qwen_image` (`models/qwen_image.py`) aplica `gradient_accumulation_steps`, `lr_scheduler`+`lr_warmup_steps` e `lora.optimizer` via `_create_optimizer`/`_create_lr_scheduler` (antes ignorava os três: 1 passo por micro-batch, LR constante, sempre AdamW8bit). Progresso/ETA/logs de step ficam locais ao run; `epochs` = épocas adicionais, e o resume na web pré-preenche `(params.epochOffset ?? 0) + params.epochs - newOffset` (mín. 1).
- **Checkpoint por época (`models/loop.py`; `qwen_image` no loop próprio com a mesma regra)**: `checkpoint_interval` conta épocas; salva uma vez por época, após o flush de gradient accumulation e o `epoch_complete`, quando `local_epoch_idx % checkpoint_interval == 0` ou na última época (sempre); `adapter_epoch_NNN` usa a época absoluta (`epoch_offset` + local) e o prune mantém os 2 mais recentes. Antes o Qwen-Image-2.1 salvava por batch (`batch_idx % checkpoint_interval`), sobrescrevendo o mesmo `adapter_epoch_NNN`, e o orchestrator subia só o 1º save (início da época).
- **DataLoader (`dataset.py::build_dataloader`)**: `num_workers` vem de `DIFFUSION_DATALOADER_WORKERS` (default `min(2, cpu_count)`; valor inválido cai no default); `pin_memory` ligado quando há CUDA; com workers > 0 usa `persistent_workers` e `prefetch_factor=2`. A ordem é determinística pela seed (`BucketBatchSampler(seed=)` com bucketing, `torch.Generator().manual_seed(seed)` sem bucketing).
- **Sincronização GPU→CPU (`models/loop.py`)**: loss e grad_norm ficam como tensores pendentes e só viram `float` (`.item()`) em `_sync_unconsumed`, chamado a cada passo de otimização (passos de difusão levem segundos; o custo do sync é irrelevante), sem sync por micro-step.
- **Telemetria e console por passo (`common_pkg/metrics.py::emit_training_step`; `models/loop.py` e `models/qwen_image.py`)**: cada passo de otimizador (step = passo de otimizador, não micro-batch) emite um registro `phase=training` em `metrics.jsonl`/`telemetry.jsonl` (step, loss, lr, grad_norm, progresso, `eta_s`/`eta_formatted`, `step_time_s` em EMA que exclui amostras/checkpoints, VRAM) e imprime no stdout, no formato do engine-kit, `[TELEMETRY] [TRAINING] (NN%) | VRAM: X.XGB Época e/E · Step s/S · Loss: L · T.Ts/step · ETA: 1m 35s` (`engine_kit.telemetry.format_eta`). Essa linha é o que aparece no console do job (`logs/run.log`).
- **Quantização e cache do transformer Qwen-Image (`models/qwen_image.py`, `loaders/quant_cache.py`)**: o transformer segue `lora.quantization`: `4bit` ⇒ NF4 + double quant, `8bit` ⇒ bnb 8-bit, `none` ⇒ sem quantização; `2bit`/`6bit` falham explicitamente ("Qwen-Image-2.1 suporta só none/4bit/8bit"). A metadata do adapter grava `quantization`. O transformer quantizado fica persistido localmente no nó em `/data/outputs/.cache/quantized/` se existir, senão `/outputs/.cache/quantized/`, com fallback `~/.cache/hephaestus/quantized` (`get_quant_cache_root`), gravado de forma atômica (`save_atomic_dir`). Diretório e validação levam em conta a identidade do checkpoint Comfy (path + size + mtime + md5 do 1º MiB), o formato (ex.: `4bit-nf4dq`; caches FP4 antigos são invalidados) e a versão do bitsandbytes; cache inválido ou corrompido é apagado e a quantização é refeita.
- **Text encoder e amostras do Qwen-Image-2.1 (`models/qwen_image.py`)**: o text encoder Qwen3-VL-8B é sempre NF4, independente de `lora.quantization`. O embed do prompt de amostra é pré-computado no `TextEmbedsCache` junto das captions; em seguida o text encoder é descarregado de fato (`_release_prompt_encoder` + variável local zerada + `cleanup_cuda()`, com `_log_vram` antes/depois). A amostra baseline roda após o unload e todas as amostras usam o embed cacheado (o text encoder não participa); o VAE da amostra liga `enable_tiling()` a partir de 1024² px (`TILE_DECODE_ABOVE_PIXELS`). VRAM medida (RTX 3060 12 GB, 768/r16): `4bit` (NF4) pico 9281 MiB, loop ~6,5 GiB → `vram-table` Qwen train `vram_min_gb: 8` (exige 10 com headroom; teste `tabela_real_difusao_elege_gpu_12gb`). `8bit` e `none` não cabem em 12 GB: `8bit` dá OOM ao carregar o text encoder (transformer 8-bit ~7 GB) e `none` dá OOM ao carregar o transformer bf16.
- **Nome e pasta das amostras do Qwen-Image-2.1 (`models/qwen_image.py`)**: as amostras são gravadas em `samples/sample_epoch_NNN.png` dentro do output, como no loop compartilhado (`models/loop.py`): baseline em `sample_epoch_000.png` e amostra por época em `sample_epoch_{epoch:03d}.png` com a época absoluta (`epoch_offset` + local). Só `samples/` é listado/subido pelo orchestrator. Em resume (`epoch_offset > 0`) não há baseline, igual ao loop compartilhado.
- **Cache de latents do VAE (`common_pkg/latent_cache.py`; flux, sd15, sdxl via `models/loop.py::_setup_latent_cache`, qwen_image em `models/qwen_image.py`)**: antes do treino, a distribuição `latent_dist` de cada imagem+bucket é pré-computada uma vez (`prepare_latent_cache`, fase `caching_latents` na telemetria, rótulo "Cacheando Latents" no `JobProgressLive.tsx`) e gravada atomicamente em `outputs/<job>/latents_cache/<namespace>/<key>.pt`: `mean` em fp32 e `std` no dtype do treino. `<key>` = hash do conteúdo do arquivo + largura×altura do bucket; `<namespace>` = hash de arch, `model_id`, fingerprint do VAE (config + nome/shape dos pesos), identidade do checkpoint custom (não no qwen_image), dtype, `LATENT_CACHE_SCHEMA_VERSION` (=2; v1 arredondava `mean`) e id do pré-processamento (`rgb-bilinear-pm1-v1`). Entrada corrompida = miss e recomputa. No loop, o dataset entrega `latent_mean`/`latent_std` e os latents são amostrados `mean + std * randn` com o RNG do treino (`latents_from_batch`/`sample_latent_dist`); scaling/shift/BN seguem nos adapters. Com cache ativo o VAE vai para a CPU e só volta à GPU para gerar amostras (`vae_on_device` no `loop.py`; no qwen_image o VAE é devolvido à CPU após cada amostra). Chave `cache_latents` (default `true`, no topo do cfg ou em `lora`) existe só na config — não há campo no wire público. Se o pré-cómputo falhar (encode ou escrita), loga `[WARN]` e o treino segue com pixels e VAE online. O dataset não tem augmentação aleatória hoje (resize bilinear determinístico); se surgir, o cache precisa ser desligado (`resolve_cache_latents(augmentation_active=True)`). O diretório é removido pela purga de outputs do orchestrator.

---

## Contratos e Execução

- **Configuração de Treino/Geração**: Esquemas de parâmetros e respostas documentados no OpenAPI canônico em `packages/contracts/openapi.yaml`.
- **Modo Mock (`ENGINE_MOCK=1`)**: Não importa dependências pesadas de CUDA nem aloca memória gráfica; gera imagens sintéticas com padrões geométricos determinísticos através de `engine_kit.mock.seed_bytes`.

---

## Adicionando um Novo Modelo

Para o checklist passo a passo transversal (Contracts, Engines, Services, Apps, Infra), consulte o guia canônico:
- [`docs/engines/novo-modelo.md`](novo-modelo.md)
