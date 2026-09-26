# Auditoria de Memória — `qwen_image.py`

**Arquivo:** `engines/trainer-difusao/src/trainer_difusao/models/qwen_image.py`  
**Data:** 2026-09-25  
**Status:** Pendente implementação

---

## Problemas Identificados

### 🔴 #1 — `inspect.signature` dentro do loop de treino (L621)

```python
# Executa N_batches × N_epochs — alocação por chamada, não cacheado
import inspect
trans_sig = inspect.signature(transformer.forward)
```

**Fix:** mover para fora do loop, após `transformer.add_adapter`:

```python
import inspect as _inspect
_trans_sig_params = set(_inspect.signature(transformer.forward).parameters)
# no loop: substituir `trans_sig.parameters` por `_trans_sig_params`
```

---

### 🔴 #2 — `img_mask` silenciosamente errado quando `ipm_item` é `None` (L603-641)

`prompt_cache` é sempre populado com 3 elementos `(pe, mask, ipm)` (L206), mas `ipm` pode ser `None`.  
Quando `pad_mask_list` fica vazio, `trans_kwargs["img_mask"]` usa `torch.zeros(...)` (L638) em vez da máscara real — **corrupção silenciosa de gradiente**, sem crash.

**Fix:** garantir que `pad_mask_list` só é usado no path `img_mask` quando len == bsz; caso contrário,
construir máscara de uns (não zeros) como fallback neutro:

```python
if pad_mask_list and len(pad_mask_list) == bsz:
    base_pad_mask = torch.cat(pad_mask_list, dim=0)
else:
    # fallback neutro: todos os tokens de texto são válidos
    base_pad_mask = torch.ones((bsz, batch_embeds.shape[1]), device=device, dtype=torch.bool)
```

---

### 🔴 #3 — `del` incompleto no loop interno deixa tensores na VRAM (L672)

```python
# atual — faltam: noise, noisy_latents, noisy_in, u, timesteps, sigmas, latents
del pred, pred_img, packed_target, target, loss, trans_kwargs, batch_embeds, embed_list, mask_list, pad_mask_list
```

**Fix:**

```python
del pred, pred_img, packed_target, target, loss, trans_kwargs, \
    batch_embeds, embed_list, mask_list, pad_mask_list, \
    noise, noisy_latents, noisy_in, u, timesteps, sigmas, latents
```

O nulling manual no final da época (L784-800) tenta compensar, mas só corre depois de todos os batches — OOM pode ocorrer no meio da época.

---

### 🟠 #4 — `alpha_channel` alocado por imagem na pré-computação de latents (L352-353 e L553-558)

```python
alpha_channel = torch.ones((1, 1, *pv.shape[2:]), device=device, dtype=pv.dtype)
pv = torch.cat([pv, alpha_channel], dim=1)
```

Canal alpha é sempre `1.0`. Realocar por imagem é desperdício.

**Fix:** lazy-init um tensor reutilizável por shape fora do loop:

```python
_alpha_cache: dict[tuple, torch.Tensor] = {}

def _get_alpha(shape, device, dtype):
    key = (shape, device, str(dtype))
    if key not in _alpha_cache:
        _alpha_cache[key] = torch.ones(shape, device=device, dtype=dtype)
    return _alpha_cache[key]
```

---

### 🟠 #5 — `torch.cat([x])` sem motivo quando `batch_size=1` (L617-618)

```python
batch_embeds = torch.cat(embed_list, dim=0) if embed_list else None
batch_mask  = torch.cat(mask_list,  dim=0) if len(mask_list) == len(embed_list) else None
```

`torch.cat` com lista de um elemento ainda aloca + copia.

**Fix:**

```python
batch_embeds = embed_list[0] if len(embed_list) == 1 else torch.cat(embed_list, dim=0)
batch_mask   = mask_list[0]  if len(mask_list)  == 1 else (torch.cat(mask_list, dim=0) if len(mask_list) == len(embed_list) else None)
```

---

### 🟠 #6 — `latents_cache` pode ser noop se Dataset não retornar `index` (L333, L539, L546)

```python
indices = batch.get("index", None)  # campo opcional
if idx_list and all(idx in latents_cache for idx in idx_list):
    latents = torch.cat([latents_cache[idx].to(device) for idx in idx_list], dim=0)
# else: re-encode via VAE — cache é inútil
```

Se `DiffusionDataset.__getitem__` não retornar `{"index": idx}`, toda iteração re-encode.  
**Verificar:** `dataset.py` — `__getitem__` deve incluir `"index": idx` no dict retornado.  
Se não incluir, o cache de ~N_samples latents ocupa RAM sem benefício algum.

---

### 🟡 #7 — Cleanup assimétrico no `except` deixa tokenizer em RAM (L277-291)

O path `except` zera `text_pipeline.text_encoder` e `components`, mas não `text_pipeline.tokenizer`
nem `text_pipeline.processor` — diferente do path feliz (L238-253) que cobre os três.

**Fix:** extrair função local chamada em ambos os branches:

```python
def _unload_text_pipeline(pipe) -> None:
    for attr in ("text_encoder", "tokenizer", "processor"):
        if hasattr(pipe, attr):
            setattr(pipe, attr, None)
    for k in list(getattr(pipe, "components", {}).keys()):
        try:
            setattr(pipe, k, None)
        except Exception:
            pass
```

---

## Tabela de Prioridade

| # | Sev | Localização | Impacto |
|---|-----|-------------|---------|
| 1 | 🔴 Perf | L621 `inspect.signature` no loop | CPU waste por step |
| 2 | 🔴 Correctness | `img_mask` fallback zeros | Gradiente corrompido silenciosamente |
| 3 | 🔴 VRAM | `del` incompleto pós-forward | OOM em runs longos |
| 4 | 🟠 VRAM | `alpha_channel` por imagem | N allocs + copies evitáveis |
| 5 | 🟠 Perf | `torch.cat([x])` bs=1 | Copy desnecessária por step |
| 6 | 🟠 Logic | `latents_cache` depende de `index` no batch | Cache pode ser noop total |
| 7 | 🟡 RAM | Cleanup assimétrico no `except` | Tokenizer vaza em RAM pós-falha |

---

## Critérios de Aceitação

- [ ] `inspect.signature` chamado exatamente uma vez, resultado em `_trans_sig_params: set[str]`
- [ ] Fallback de `img_mask` usa `ones` (neutro) em vez de `zeros`
- [ ] `del` pós-forward inclui todos os tensores intermediários do passo
- [ ] Alpha channel reutilizado por shape em vez de realocado por imagem
- [ ] `torch.cat` guarded para `len == 1`
- [ ] Confirmado que `DiffusionDataset.__getitem__` retorna `"index"` no dict
- [ ] `_unload_text_pipeline` extrai e unifica os dois paths de cleanup
