# ADR-0021 — Text encoder offload on cache ready (adr-difusao-vram)

- **Status:** Aceito e implementado (2026-09-23)
- **Data:** 2026-09-23
- **Componentes:** `engines/trainer-difusao/common_pkg/text_embeds.py`, pipelines FLUX/SD15/SDXL, `services/orchestrator`
- **Fontes:** Análise de VRAM com text encoders carregados em GPU após pré-computação do cache de embeddings.

## Contexto

Durante treinos LoRA com cache de text embeddings habilitado:
1. Pré-compute roda uma vez no início (epoch 0) → text encoders na GPU.
2. Loop de treino usa cache hits → embeddings carregados de disco (CPU) e movidos diretamente para `device`.
3. Text encoders **continuam na VRAM** durante todas as épocas de treino se não forem descarregados, desperdiçando ~2–8 GB de VRAM (dependendo do modelo: ~2 GB em SD 1.5, ~3-4 GB em SDXL, ~8-10 GB em FLUX).

### Desafios Identificados
- Deletar o objeto (`del enc`) em Python é ineficaz se referências continuarem no escopo do chamador, e causa crashes fatais caso o encoder seja necessário posteriormente.
- Text encoders continuam sendo necessários durante o treino para:
  1. Geração de amostras visuais de validação (`sample_prompt` na época baseline 0 e em épocas periódicas via `_generate_sample_*`).
  2. Cache miss eventual em `_cached_encode`.
- Retomada de treino (`epoch_offset > 0`): deve ignorar o descarregamento caso solicitado.
- O orchestrator (`services/orchestrator`) roda containers via Docker CLI com lista explícita de variáveis de ambiente (`-e ...`). Se não propagar `ENABLE_TEXT_ENCODER_UNLOAD`, a flag não alcança os containers.

## Decisão

**D0 — Implementar CPU Offloading e Context Manager `_temporary_device_encoders` controlado por `ENABLE_TEXT_ENCODER_UNLOAD`**

1. **CPU Offload em vez de Deletion:**
   - Em `_offload_encoders_to_cpu(encoders)`, os modelos chamam `.to("cpu")` seguido de `torch.cuda.empty_cache()` e `gc.collect()`.
   - Isso libera 100% da VRAM alocada pelos encoders de texto mantendo os objetos intactos na memória RAM do sistema.

2. **Hook de Reload com Context Manager:**
   - Implementado `@contextlib.contextmanager def _temporary_device_encoders(encoders, device)`:
     - Quando ativo, move os encoders para `device`, executa o bloco e, no `finally`, devolve-os para `"cpu"` limpando o cache CUDA.
     - Utilizado em `_generate_sample_sd15`, `_generate_sample_flux` e `_generate_sample_sdxl` tanto na baseline quanto nas épocas periódicas.
     - Utilizado em `_cached_encode` no caso de cache miss inesperado.

3. **Política de Retomada:**
   - `should_unload = ENABLE_TEXT_ENCODER_UNLOAD and epoch_offset == 0` aplicado em SD 1.5, FLUX e SDXL.

4. **Propagação no Orquestrador e Infra:**
   - `services/orchestrator/src/app/mod.rs`: repassa `ENABLE_TEXT_ENCODER_UNLOAD` para o executor Docker one-shot em jobs de difusão.
   - `services/orchestrator/src/main.rs`: repassa `ENABLE_TEXT_ENCODER_UNLOAD` para o launcher do daemon de difusão.
   - `infra/compose.yaml`, `infra/compose.gpu.yaml` e `infra/env.gpu.example`: mapeiam e documentam a variável.

## Consequências

### Benefícios
1. **Economia real de VRAM:** ~2–8 GB liberados durante as etapas de backward pass e forward pass do modelo principal.
2. **Resiliência:** Nenhuma quebra em geração de amostras ou cache miss; encoders sobem para GPU apenas quando estritamente necessário e retornam para CPU.
3. **Zero breaking change:** Padrão desabilitado (`ENABLE_TEXT_ENCODER_UNLOAD=0` / `false`).
4. **Governança:** Propagação ponta a ponta documentada e testada.
