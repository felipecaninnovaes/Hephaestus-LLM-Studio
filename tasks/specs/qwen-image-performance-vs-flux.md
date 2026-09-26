# Diagnóstico — Lentidão de Treino Qwen-Image-2.1 vs Flux.2 Klein

**Data:** 2026-09-25  
**Escopo:** Comparação de pipeline de treino LoRA entre `qwen_image.py` e `flux.py`

---

## Veredicto

A lentidão **não é só arquitetural** — há implementação subótima real no Qwen que
não existe no Flux. O modelo é modestamente mais pesado, mas o pipeline adiciona
overhead evitável que multiplica o custo por step.

---

## Tabela Comparativa por Dimensão

| Dimensão | Flux.2 Klein | Qwen-Image-2.1 | Delta |
|----------|-------------|----------------|-------|
| **Parâmetros DiT** | ~2.8 B (Klein) | ~7 B | ~2.5× mais params |
| **LoRA target_modules** | 12 módulos | 7 módulos | Flux tem mais targets |
| **Seq len de atenção** | `txt_len + img_patches` (sem máscara bool) | `txt_len + img_patches` + `img_mask` bool | Similar |
| **VAE no loop** | on-the-fly a cada step (GPU full-time) | pré-computado em RAM + VAE offload para CPU | Qwen é melhor aqui |
| **Text embeds** | `TextEmbedsCache` em disco (.pt, bs=32) | `dict` em RAM, bs=8 GPU / bs=4 CPU | Flux usa bs 4× maior |
| **`inspect.signature` por step** | ❌ nunca | ✅ toda iteração (L621) | Qwen tem overhead puro |
| **`autocast`** | ❌ (dtype estático) | ✅ `torch.cuda.amp.autocast` por step | Qwen tem wrapper extra |
| **Gradient checkpointing** | ✅ `use_reentrant=False` | ✅ (mas via `try/except` genérico) | Ambos têm |
| **Forward kwargs** | dict literal hardcoded | dict dinâmico + `inspect` a cada step | Qwen tem overhead |
| **`torch.cat` em embed_list** | n/a | por step sem guard bs=1 | Qwen aloca desnecessariamente |
| **`alpha_channel` torch.ones** | n/a | realocado por imagem no pré-compute | Overhead de setup |
| **DataLoader num_workers** | 0 | 0 | Ambos ruins |
| **DataLoader pin_memory** | False | False | Ambos ruins |
| **Pré-compute text bs=32** | ✅ | ✅ bs=8/4 (4-8× menor) | Qwen é mais lento |

---

## Causa Raiz dos Overheads Implementação (não arquiteturais)

### 1. `inspect.signature` por step — ~5-15ms/step desperdiçado

```python
# qwen_image.py L621 — dentro do loop de batch
import inspect
trans_sig = inspect.signature(transformer.forward)
```

Em Flux: kwargs são construídos com condicionais simples (`if is_flux2`), sem introspecção.
Em 1000 steps com dataset pequeno, isso é ~5-15s de CPU puro descartados.

### 2. Pré-computação de text embeddings 4-8× mais lenta

- Flux: `TextEmbedsCache` com bs=32 em GPU
- Qwen: bs=8 em GPU (ou bs=4 em CPU se falhar 4-bit)

Para 200 prompts únicos: Flux faz 7 batches, Qwen faz 25-50. A diferença escala com o dataset.

### 3. VAE on-the-fly vs cache de latents

Este é o único ponto onde **Qwen é melhor que Flux**:
- Qwen pré-computa todos os latents em RAM e descarrega o VAE da GPU
- Flux roda `vae.encode()` em toda iteração de toda época

Para 10 épocas com 100 imagens: Flux faz 1000 forward passes no VAE; Qwen faz 100 (1×).
Mas o cache de latents do Qwen ocupa RAM (para dataset de 100 imgs @ 1024px ≈ ~800 MB).

### 4. `torch.cuda.amp.autocast` no forward — overhead de context manager

Flux não usa `autocast` — opera em `bfloat16` estático. Qwen entra/sai de um context
manager de autocast por step. Em GPU moderna com bfloat16 nativo, o autocast não
acelera nada além de adicionar ~0.1ms de overhead de Python por step.

### 5. DataLoader sem `num_workers` e sem `pin_memory` (ambos)

```python
# common_pkg/core.py (build_dataloader)
num_workers=0, pin_memory=False
```

`num_workers=0` significa I/O de imagem bloqueante no main thread — o DataLoader lê
a imagem do disco **enquanto a GPU está ociosa esperando o próximo batch**.
Com `num_workers=2` e `pin_memory=True`, a GPU ficaria ocupada enquanto o próximo
batch é preparado em paralelo. Isso afeta **todos** os trainers igualmente, mas
Qwen é mais sensível porque tem forward pass mais pesado (maior idle relativo).

---

## Separando Arquitetural de Implementação

### O que é genuinamente mais lento por arquitetura
- **7B vs 2.8B params**: backward pass ~2.5× mais lento em FLOPs brutos
- **Single-Stream DiT com sequência conjunta txt+img**: atenção cruzada sobre sequência
  maior que o Flux.2 Klein (que usa arquitetura dual-stream com texto mais curto)
- **Qwen3-VL como text encoder**: mais pesado que Qwen3 mini do Flux.2 Klein

### O que é bug/subimplementação evitável

| Problema | Custo estimado por epoch (100 imgs, 10 epochs) |
|----------|-----------------------------------------------|
| `inspect.signature` no loop | ~5-15s CPU |
| Text embed bs=8 vs bs=32 | ~3-4× mais lento na fase de pré-computação |
| `autocast` context manager | ~100ms/1000 steps (menor) |
| `torch.cat([x])` sem guard bs=1 | ~10ms/1000 steps (menor) |
| `alpha_channel` realocado por imagem | ~20ms no pré-compute (menor) |

**Estimativa conservadora:** os itens de implementação somam 20-40% de overhead
evitável sobre o tempo total de treino, com o `inspect.signature` e o batch size
de pré-computação sendo os mais impactantes.

---

## Fixes Prioritários para Velocidade

Em ordem de impacto:

### P0 — `inspect.signature` fora do loop (minutos de ganho em runs longos)
```python
# Após transformer.add_adapter(lora_config), antes do loop:
import inspect as _inspect
_trans_sig_params = set(_inspect.signature(transformer.forward).parameters)

# No loop, substituir:
# if "encoder_hidden_states_mask" in trans_sig.parameters:
if "encoder_hidden_states_mask" in _trans_sig_params:
```

### P1 — Aumentar batch size de pré-computação de texto
```python
# Atual: bs = 8 if use_cuda else 4
bs = 32 if use_cuda else 8  # alinhado ao TextEmbedsCache do Flux
```
O fallback de OOM já existe (divide por 2) — só o ponto de partida é conservador demais.

### P2 — Remover `autocast` desnecessário
```python
# Atual:
with torch.cuda.amp.autocast(dtype=target_dtype) if device == "cuda" else nullcontext():
    pred = transformer(**trans_kwargs)[0]

# Fix: se target_dtype já é bfloat16/float16, autocast não ajuda
# Remover o autocast e garantir que os tensores de entrada já estão no dtype correto
pred = transformer(**trans_kwargs)[0]
```
Atenção: validar que `packed_noisy`, `batch_embeds` e `timesteps` estão todos em
`target_dtype` antes de remover.

### P3 — `num_workers=2, pin_memory=True` no DataLoader
```python
# common_pkg/core.py — build_dataloader
DataLoader(dataset, ..., num_workers=2, pin_memory=torch.cuda.is_available(),
           prefetch_factor=2, persistent_workers=True)
```
Cuidado: `persistent_workers=True` requer `num_workers > 0`.

### P4 — Migrar pré-computação para `TextEmbedsCache` em disco
Remove a dependência de RAM para datasets grandes e permite reuso entre runs.
Já existe em `common_pkg/text_embeds.py` — é o mesmo padrão do Flux.

---

## O que NÃO mudar

- O cache de latents em RAM é um diferencial positivo do Qwen vs Flux — manter.
  (Flux roda VAE on-the-fly a cada step, o que é pior para epochs > 1)
- Gradient checkpointing está correto — não remover mesmo que cause lentidão,
  pois a RTX 3060 12 GB não comporta o backward do 7B sem ele.
