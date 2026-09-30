# Spec — Otimização e Modularização das Engines

## 1. Contexto e Motivação
Historicamente, novos modelos suportados em `engines/trainer-difusao` (`sd15`, `sdxl`, `flux`, `qwen_image`) foram criados duplicando e adaptando rotinas procedurais completas ("god procedures" de até 1.400 linhas em `flux.py`). Isso gerou duplicação maciça de código em:
- Carregamento e quantização de transformadores e text encoders com persistência em disco.
- Geração de amostras periódicas com callbacks de telemetria.
- Salvamento atômico de adaptadores e checkpoints LoRA (`.tmp_*` seguido de `os.replace`).
- Liberação e gerenciamento de VRAM/RAM.

## 2. Decisões Arquiteturais

### A. Subpacote `trainer_difusao.loaders`
- `quant_cache.py`: Centraliza validação e metadados de cache quantizado (`_is_cache_valid`, `_save_quant_metadata`). Re-exportado em `trainer_difusao.models.flux_pkg.quant_cache` para manter total retrocompatibilidade.
- `transformer_loader.py`: Encapsula carregamento de DiT / UNet, quantização BitsAndBytes / torchao intx, cache em disco e aplicação de pesos customizados.
- `text_encoder_loader.py`: Encapsula carregamento de Qwen3-VL, T5-XXL e CLIP, quantização e merge de pesos soltos.
- Consumido tanto pelo treino (`flux.py`) quanto pela geração (`generation/runner.py`).

### B. Subpacote `trainer_difusao.models.sd_pkg`
- `embeddings.py`: `_compute_sdxl_embeddings`.
- `sample.py`: `_generate_sample_sd15` e `_generate_sample_sdxl`.
- Re-exportado em `sd15.py` e `sdxl.py` mantendo os contratos consumidos por `train.py`.

### C. Utilitários Canônicos em `common_pkg/lora_io.py`
- `save_adapter_checkpoint`: Cria diretório `checkpoints/` e grava de forma atômica `{base_name}_epoch_{epoch:03d}.safetensors`.
- `save_final_adapter`: Grava de forma atômica `{base_name}.safetensors` e, se necessário, replica atomicamente para `adapter.safetensors` via `.tmp_adapter.safetensors` e `os.replace`.

### D. Utilitários Canônicos em `engine-kit`
- `engine_kit.vram.vram_guard`: Context manager para monitoramento e limpeza automática de VRAM via `cleanup_cuda`.
- `engine_kit.vram.release_memory`: Dispara `gc.collect()`, `malloc_trim(0)` e esvaziamento de cache CUDA IPC.
- Arquivos `.dockerignore` dedicados em cada pasta de engine (`trainer-difusao`, `trainer-yolo`, `trainer-clip`).

## 3. Critérios de Aceite e Verificação
- Paridade 100% em todos os testes unitários das engines (363 testes passando: 224 difusao, 107 yolo, 27 kit, 5 clip).
- Salvamento atômico garantido para todos os checkpoints e adaptadores.
- Descarregamento explícito de text encoders volumosos (Qwen2.5-VL) preservando regras de VRAM de `docs/PITFALLS.md`.
