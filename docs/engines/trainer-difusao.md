# Trainer Difusão (`engines/trainer-difusao`)

O `trainer-difusao` é o motor de difusão do Hephaestus, responsável pelo treinamento de adapters LoRA/QLoRA e execução de inferência em modelos generativos de imagem. Suporta execução real acelerada por GPU (via PyTorch, Diffusers e PEFT) e execução simulada determinística (`ENGINE_MOCK=1`).

---

## Arquitetura Modular

O pacote foi desenhado para desacoplar modelos, pipelines de inferência e ciclo de vida do daemon:

### 1. `models/` e `models/flux_pkg/`
Implementações de treinamento organizadas sob a abstração `BaseModelTrainer`:
- **`FluxTrainer` / `flux_pkg/`**: Suporte ao FLUX.2 Klein 4B. Implementa `rope.py` (Rotary Positional Embeddings 3D), `quant_cache.py` (quantização de pesos e cache), `encoding.py` e rotinas especializadas de amostragem em `sample.py`.
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
- **`text_encoder.py`**: Suporte para text encoders oficiais e text encoders customizados definidos no catálogo.
- **`progress.py`**: Callback de progresso por timestep/amostragem reportado em tempo real ao `engine-kit.telemetry`.

### 4. `serve_pkg/` e `serve.py`
Daemon HTTP de longa duração para inferência interativa, gerenciado pelo orchestrator:
- **Endpoints**: `/health` (status, spec carregada, uso de VRAM), `/generate` (execução da geração) e `/shutdown` (encerramento gracioso).
- **`state.py`**: Mantém o modelo aquecido em memória (`loaded_spec`), gerencia trava de concorrência (`_busy`) para garantir inferência atômica e computa tempos de atividade.

---

## Treino LoRA e QLoRA

- **Adapters PEFT**: Injeta adaptadores de baixo posto (*Low-Rank Adaptation*) nas camadas lineares e de atenção dos modelos base.
- **Quantização BitsAndBytes (`quantization.py`)**: Suporte a quantização de 4-bit (`nf4`, `fp4`) e 8-bit (`int8`), permitindo o fine-tuning de modelos pesados em GPUs de consumo.
- **Configurações e Limites de VRAM**: Os limites operacionais de VRAM para cada modelo e tipo de quantização são canônicos em `packages/policies/vram-table.yaml`.
- **Resume e LR (`TrainingLoopRunner`: flux, sd15, sdxl)**: `learningRate` do resume é o pico da curva original; o scheduler usa horizonte `steps_per_epoch * (epoch_offset + epochs)` com o warmup configurado e é posicionado em `steps_per_epoch * epoch_offset` (`_create_lr_scheduler(..., last_step=)` em `optimizers.py`), continuando o LR exatamente de onde o run interrompido parou (warmup já consumido não se repete). Progresso/ETA/logs de step ficam locais ao run; `epochs` = épocas adicionais, e o resume na web pré-preenche `(params.epochOffset ?? 0) + params.epochs - newOffset` (mín. 1). `qwen_image` usa LR constante (sem scheduler).
- **DataLoader (`dataset.py::build_dataloader`)**: `num_workers` vem de `DIFFUSION_DATALOADER_WORKERS` (default `min(2, cpu_count)`; valor inválido cai no default); `pin_memory` ligado quando há CUDA; com workers > 0 usa `persistent_workers` e `prefetch_factor=2`. A ordem é determinística pela seed (`BucketBatchSampler(seed=)` com bucketing, `torch.Generator().manual_seed(seed)` sem bucketing).
- **Sincronização GPU→CPU (`models/loop.py`)**: loss e grad_norm ficam como tensores pendentes e só viram `float` (`.item()`) em `_sync_unconsumed`, chamado nos passos de log (a cada 5 passos de otimização ou no fim da época), sem sync por micro-step.
- **Cache de quantização do Qwen-Image (`models/qwen_image.py`, `loaders/quant_cache.py`)**: o transformer 4-bit fica persistido localmente no nó em `/outputs/.cache/quantized/` (ou `/data/outputs/...`, fallback `~/.cache/hephaestus/quantized`), gravado de forma atômica (`save_atomic_dir`). Diretório e validação levam em conta a identidade do checkpoint Comfy (path + size + mtime + md5 do 1º MiB), o modo (`4bit`) e a versão do bitsandbytes; cache inválido ou corrompido é apagado e a quantização é refeita.

---

## Contratos e Execução

- **Configuração de Treino/Geração**: Esquemas de parâmetros e respostas documentados no OpenAPI canônico em `packages/contracts/openapi.yaml`.
- **Modo Mock (`ENGINE_MOCK=1`)**: Não importa dependências pesadas de CUDA nem aloca memória gráfica; gera imagens sintéticas com padrões geométricos determinísticos através de `engine_kit.mock.seed_bytes`.

---

## Adicionando um Novo Modelo

Para o checklist passo a passo transversal (Contracts, Engines, Services, Apps, Infra), consulte o guia canônico:
- [`docs/engines/novo-modelo.md`](novo-modelo.md)
