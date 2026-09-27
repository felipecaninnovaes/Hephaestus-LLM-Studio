# Spec — Qwen-Image-2.1 Native Rewrite (cutover do fallback diffusers)

**Status: caminho de treino concluído/verificado/mergeable; daemon de
geração explicitamente fora de escopo nesta fatia.** Branch
`feat/qwen-image-2-1-native`. `models/qwen_image.py` (treino nativo) está
completo e verificado: `uv run pytest -q` dentro de `engines/trainer-difusao`
com `ENGINE_MOCK=1` → 234 passed; smoke real em GPU (RTX 3060) completou 1
época real de treino (`metrics.jsonl`: `epoch_complete`,
`loss=0.026513001415878534, step=8, epoch=1`), checkpoint LoRA-only válido
(256 tensores, 100% `lora` no nome, 67.139.000 bytes) e amostras
`baseline.png`/`epoch1.png` (PNG RGB 512x512 válidos). O daemon quente de
geração (`generation/runner.py`, branch `elif base_model ==
"qwen-image-2.1":`) tinha um gap funcional achado pelo `@reviewer`: chamava
`QwenImage21Pipeline.from_pretrained(...)` (classe que não existe com esse
construtor) e, mesmo corrigido, o restante do `runner.py` downstream exige
uma interface completa equivalente a `diffusers.DiffusionPipeline`
(`.load_lora_weights()`/`.set_adapters()` multi-adapter hot-swap, dict
`.components` para reconstrução de variante img2img, `.scheduler.config`,
`pipe(**kwargs).images[0]` chamável) que o `QwenImage21Pipeline` vendorizado
(sampler de preview de treino, ~30 linhas) não fornece. Este branch foi
trocado por um erro explícito (`_die(...)`) em vez de ficar quebrado
silenciosamente; a implementação completa do daemon de geração nativo foi
movida para uma fatia futura separada rastreada em
`tasks/backlog.md` ("Daemon de geração Qwen-Image-2.1 nativo").

---

## Objetivo

`engines/trainer-difusao` alega treinar "Qwen-Image-2.1", mas o código atual
(`models/qwen_image.py`, `qwen_pkg/sample.py`, `generation/runner.py`)
resolve a classe do pipeline via
`getattr(diffusers, "QwenImage21Pipeline", getattr(diffusers, "QwenImagePipeline", None))`.
A biblioteca `diffusers` **nunca** distribuiu classes `QwenImage21*`, então
esse fallback sempre cai silenciosamente para a arquitetura antiga
Qwen-Image (1.0), enquanto o sistema rotula o resultado como 2.1. Isto é um
**bug funcional**, não uma questão de estilo.

## Bug atual (evidência)

- `models/qwen_image.py`, `qwen_pkg/sample.py`, `generation/runner.py`: todos
  resolvem a classe do pipeline via `getattr(diffusers, "QwenImage21Pipeline", …)`
  com fallback silencioso para `QwenImagePipeline` (1.0), pois `diffusers`
  jamais publicou as classes 2.1.
- Resultado: todo treino/serving rotulado "2.1" hoje executa, na prática, a
  arquitetura 1.0.

## Arquitetura-alvo (referência confirmada correta e completa)

Localizada em `tmp/ai-toolkit/extensions_built_in/diffusion_models/qwen_image_2/`
(ler os arquivos diretamente, não parafrasear):

- **`qwen_image_2.py`** — `QwenImage2Model(BaseModel)`, ~920 linhas:
  - `load_model` (L152-205): lógica de resolução de fonte HF.
  - `get_noise_prediction` (L459-513): core do train step.
  - `get_loss_target` (L515-519): `noise - latents` (velocity), sem
    time-flip.
  - Tratamento de imagem de controle:
    `_normalize_control_images` → `_prepare_control_images` →
    `encode_condition_images`.
  - VAE com canal alpha RGBA.
  - `get_bucket_divisibility` — `VISION_TOKEN_PIXELS` = 16 (VAE spatial) × 2
    (agrupamento de token latente).
  - Flags: `encode_control_in_text_embeddings=True`,
    `has_multiple_control_images=True`.
- **`src/transformer.py`** — `QwenImage21Transformer2DModel` (L855-1267):
  DiT single-stream de 32 camadas, atenção block-causal, RoPE própria
  (`QwenImage21Rope`), KV-cache por camada
  (`QwenImage21KVLayerCache`/`QwenImage21KVCache`), AdaLN contínuo, FFN
  SwiGLU, processadores de atenção Flex + padrão.
- **`src/vae.py`** — `AutoencoderKLQwenImage21` (L1168-1872): VAE causal 3D
  (dim de frame=1 para imagens), RGBA, 16x spatial, 64 canais latentes,
  encode/decode em tiles.
- **`src/text_encoder.py`** — `QwenImage21TextEncoder(Qwen3VLTextEncoder)`:
  Qwen3-VL-8B como encoder de texto+visão, lê a última camada decoder antes
  do RMSNorm final.
- **`src/pipeline.py`** — `QwenImage21PromptEncoder` (encode de
  prompt+imagem de referência), `QwenImage21Pipeline` (sampling),
  utilitários `pack_latents`/`run_transformer`/`calculate_shift`.
- **Fonte dos pesos:** repack single-file de transformer/VAE em
  `Comfy-Org/Qwen-Image-2.1`; configs/processor/text-encoder a partir de
  `Qwen/Qwen-Image-2.1` (ver `load_model` L152-205 para a lógica exata de
  resolução — portar fielmente; sem gate de `HF_TOKEN` pois são públicos
  conforme histórico anterior de smoke do Qwen-Image-2.1).
- **T2I+edit unificado:** checkpoint único. Imagem de referência/controle →
  reserva um slot `<|image_pad|>` por imagem de referência no prompt
  (encoder Qwen3-VL), o DiT insere 4 tokens latentes VAE por slot. Texto +
  referências são modulados a partir de t=0 (`causal_condition`); apenas os
  tokens-alvo veem o timestep amostrado.

## Decisões vinculantes do usuário (não relitigar)

1. **Cutover total:** deletar completamente o caminho de fallback
   `getattr`-diffusers. Sem dual-path, sem shim para o comportamento antigo
   do Qwen-Image 1.0.
2. **Mesmo slug de arquitetura no DB/config:** `qwen-image-2.1` (ver
   `services/api-principal` migration 0019, `models_arch_check`). Sem nova
   migration. LoRAs existentes treinados sob a arquitetura antiga (errada)
   ficam incompatíveis daqui pra frente — esperado, não é regressão a
   corrigir.
3. **Orçamento de VRAM para Qwen3-VL-8B (maior que o Qwen2.5-VL-7B atual) é
   ABERTO** — re-derivar estratégia de quantização/offload do zero para RTX
   3060 12GB. Sem garantia prévia de que caiba; smoke test real em GPU
   decide viabilidade, iterar se OOM (4-bit já provado necessário para o
   encoder 7B menor, conforme `docs/PITFALLS.md` entrada "Text encoder
   VLM" — esperar tratamento igual ou mais agressivo, ex.: offload para CPU
   de camadas decoder não usadas, residência sequencial VLM/DiT).
4. **Paridade completa exigida:** TANTO o treino (`models/qwen_image.py`)
   QUANTO o caminho quente de geração (`generation/runner.py`,
   `qwen_pkg/sample.py` sample-during-training) devem ser portados para o
   novo pipeline nativo. Sem split "treino agora, geração depois".

## Convenções do repo a preservar (não reinventar)

- `models/base.py`: `BaseModelTrainer.train(cfg: dict, output: Path) -> None`
  é a ÚNICA interface exigida — a Fase C da unificação de trainers decidiu
  explicitamente que `qwen_image.py` permanece um flagship standalone (NÃO
  forçado no padrão `ModelAdapter`/`TrainingLoopRunner` usado por
  sd15/sdxl/flux — física bespoke demais, confirmado pelo usuário
  anteriormente neste mesmo projeto). Manter essa decisão; não
  "adapt-ificar" como parte desta reescrita.
- `TextEmbedsCache` (cache de prompt em disco, `models/qwen_image.py` já
  usa, ver histórico da Fase C em `tasks/active.md`) — reutilizar para o
  pré-compute do novo encoder Qwen3-VL-8B (pré-compute do VLM em fase
  separada, depois descarregar antes do treino do transformer, conforme
  padrão de 2 fases atual e entrada "Text encoder VLM" de
  `docs/PITFALLS.md`).
- `TelemetryEmitter` do `engine-kit` para emissão de fase/métrica (fases
  usadas hoje: init→loading_models→setup_lora→dataset_ready→
  generating_baseline_sample→baseline_ready→training_started→training→
  epoch_complete→completed) — manter nomes/forma de fase, campos de
  telemetria ETA/VRAM (`etaSeconds`, `etaFormatted`, `stepTimeSeconds`,
  `vramReservedGb`) de `common_pkg/metrics.py`.
- Convenção de guarda de VRAM: usar `torch.cuda.mem_get_info(0)` para
  checagens reais de VRAM livre, nunca `memory_reserved()` isolado (ver
  PITFALLS "Guard de VRAM libera alocação que ainda estoura OOM").
- Sample-during-training com resolução capada
  (`min(resolution, 512)`), VAE `enable_tiling()`/`enable_slicing()`,
  `finally: torch.cuda.empty_cache()` (entradas PITFALLS para OOM em
  sampling).
- Padrões de limpeza de memória de `fix/qwen-image-memory-leaks` (já
  integrados): `pipe.components[k] = None` explícito, `del pipe`,
  `release_memory()`, `malloc_trim(0)` em `finally`;
  `unload_lora_weights()` incondicional antes de avaliar novos adapters no
  cache do daemon de geração (`generation/runner.py`).
- Naming: NUNCA nomear a variável de escala do LoRA como `alpha` quando
  também existe um tensor de canal alpha de imagem no escopo — nomear
  `lora_alpha` / `alpha_channel` respectivamente (PITFALLS: essa colisão
  exata já causou bug de shape mismatch antes).
- Equivalente de `get_bucket_divisibility` já existe para bucketing no
  pipeline de dataset atual — localizar o atual (`grep`/`graft` por uso de
  bucket divisibility) e alinhar ao `VISION_TOKEN_PIXELS` do ai-toolkit,
  compatível com o bucketing de aspect-ratio que o dataset loader atual faz
  para qwen_image.
- `packages/policies/vram-table.yaml` e `packages/policies/engines.yaml`
  são a fonte canônica de mínimos de VRAM e versões de toolchain/imagem —
  atualizar ambos para as novas fontes de peso/footprint (não deixar
  entradas obsoletas apontando para a antiga suposição
  getattr-diffusers).

## Entregáveis

1. Novos módulos vendorizados/portados em
   `engines/trainer-difusao/src/trainer_difusao/models/qwen_pkg/qwen_image_2/`
   (ou layout `qwen_pkg/` equivalente já existente — seguir convenção atual
   de pacote): `transformer.py`, `vae.py`, `text_encoder.py`, `pipeline.py`,
   portados da referência ai-toolkit e adaptados às versões de
   torch/diffusers/transformers do repo (checar `pyproject.toml`/`uv.lock`
   pela versão instalada de `transformers` — suporte a Qwen3-VL exige
   `transformers` recente; bumpar o pin se necessário e anotar).
2. `models/qwen_image.py` reescrito para chamar esses módulos nativos
   diretamente (sem `getattr(diffusers, ...)` em nenhum lugar), preservando
   a classe `QwenImageTrainer(BaseModelTrainer)` e a assinatura de
   `train()` para que o dispatch `get_trainer` de `models/__init__.py`
   permaneça intocado.
3. `generation/runner.py` (daemon quente de geração LoRA) e
   `qwen_pkg/sample.py` (sample-during-training) atualizados para usar o
   novo `QwenImage21Pipeline` nativo — remover o fallback diffusers também
   ali.
4. `packages/policies/vram-table.yaml` + `packages/policies/engines.yaml`
   atualizados para o footprint real.
5. `tests/test_qwen_image.py` totalmente reescrito (deletar testes que
   fixam o comportamento antigo de fallback diffusers — testam uma
   implementação sendo deletada, não comportamento visível ao usuário que
   valha re-pinar) para cobrir: semântica de
   get_noise_prediction/get_loss_target, tratamento de canal alpha RGBA no
   VAE, tratamento de imagem de controle/slot de token de referência,
   divisibilidade de bucket, invariantes de limpeza de VRAM (espelhar
   estrutura/convenções de teste do arquivo sendo substituído).
6. `uv run pytest` verde dentro de `engines/trainer-difusao`.
7. Smoke real em GPU (relatar evidência exata, não alegar sem rodar): 1
   epoch real de treino E 1 chamada real de geração quente no node RTX 3060
   (`dockeruser@10.15.1.2`, path `~/Hephaestus-LLM-Studio`), mesmo padrão de
   smokes anteriores do Qwen-Image-2.1 documentados em `tasks/active.md`
   (build de imagem, dataset sintético, `ENGINE_MOCK=0`, observar
   `metrics.jsonl` pelas fases esperadas, confirmar checkpoint safetensors
   válido, confirmar ausência de OOM). Se ocorrer OOM, iterar em
   quantização/offload (decisão #3 acima permite) — não reportar sucesso
   sem evidência de execução bem-sucedida.

## Não-metas

- NÃO tocar `apps/web`, `services/*`, `crates/*` — slug de arquitetura/DB
  inalterado, sem mudança de contrato.
- NÃO forçar `qwen_image.py` no padrão `ModelAdapter`/`TrainingLoopRunner`.
- NÃO manter nenhum dual-path/shim "por via das dúvidas" — cutover limpo
  conforme decisão #1.
