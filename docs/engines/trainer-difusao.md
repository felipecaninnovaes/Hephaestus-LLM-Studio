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

---

## Contratos e Execução

- **Configuração de Treino/Geração**: Esquemas de parâmetros e respostas documentados no OpenAPI canônico em `packages/contracts/openapi.yaml`.
- **Modo Mock (`ENGINE_MOCK=1`)**: Não importa dependências pesadas de CUDA nem aloca memória gráfica; gera imagens sintéticas com padrões geométricos determinísticos através de `engine_kit.mock.seed_bytes`.

---

## Adicionando um Novo Modelo

Para o checklist passo a passo transversal (Contracts, Engines, Services, Apps, Infra), consulte o guia canônico:
- [`docs/engines/novo-modelo.md`](novo-modelo.md)
