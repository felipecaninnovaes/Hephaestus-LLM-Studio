# Diagnóstico — Arquivo LoRA do Qwen-Image-2.1 menor que o esperado

**Data:** 2026-09-25  
**Sintoma:** Arquivo `.safetensors` do Qwen rank=128 fica menor que o esperado  
**Veredicto:** Bug real — o rank configurado **não está sendo aplicado corretamente**

---

## Causa Raiz: dois problemas combinados

### Problema #1 (Principal) — `add_adapter()` vs `get_peft_model()`

```python
# qwen_image.py L422
transformer.add_adapter(lora_config)   # ← ERRADO para modelo 4-bit BnB

# flux.py L495
transformer = get_peft_model(transformer, lora_config)  # ← CORRETO
```

`add_adapter()` em modelo carregado com `BitsAndBytesConfig(load_in_4bit=True)` **não**
prepara os módulos quantizados para receber injeção LoRA. A API de alto nível não chama
internamente `prepare_model_for_kbit_training()` — o PEFT docs dizem que com BnB 4-bit
é obrigatório usar `get_peft_model()` diretamente (ou preparar antes).

**Resultado:** `add_adapter()` pode silenciosamente não injetar LoRA em parte ou em
todos os módulos quantizados. O modelo treina, `loss` cai — mas está atualizando
parâmetros de layers não-quantizadas (norms, bias) em vez das projeções de atenção.
`get_peft_model_state_dict()` retorna apenas o que foi injetado → arquivo pequeno.

Nota: o código tem comentário em `flux.py` (L492-493) explicando **exatamente** por que
`prepare_model_for_kbit_training` não é chamado (quebra SDPA). O Qwen não seguiu o
mesmo padrão.

### Problema #2 (Agravante) — colisão silenciosa de chaves em `_normalize_lora_keys`

`_LORA_KEY_PREFIXES_TO_STRIP` contém prefixos sobrepostos:
```python
_LORA_KEY_PREFIXES_TO_STRIP = (
    "base_model.model.",           # genérico
    "unet.base_model.model.",
    "transformer.base_model.model.",
)
```

Se `add_adapter()` gera chaves com prefixo inesperado (ex: `model.` em vez de
`base_model.model.`), ou se dois adaptadores geram chaves que normalizam para o
mesmo nome:

```
"transformer.base_model.model.blocks.0.attn.to_q.lora_A.weight"
  → strip "transformer.base_model.model." → "blocks.0.attn.to_q.lora_A.weight"  ✅ salvo

"base_model.model.blocks.0.attn.to_q.lora_A.weight"
  → strip "base_model.model."             → "blocks.0.attn.to_q.lora_A.weight"  ❌ COLISÃO → descartado
```

A segunda ocorrência é silenciosamente descartada com `[WARN]` no log. Se o operador
não estiver monitorando stdout do container, nunca vê o aviso.

---

## Tamanhos esperados (Qwen-Image-2.1, dim≈3072, 28 blocos, 7 target_modules)

| Cenário | Tamanho |
|---------|---------|
| Rank=128 correto (100% injetado) | **~294 MB** |
| Rank=128 com 50% colisão/falha | ~147 MB |
| Rank=16 correto (fallback silencioso) | ~37 MB |
| Rank=128, 0% injetado (falha total) | ~alguns KB |

Se o arquivo que você observa é << 294 MB, o problema #1 é o culpado.  
Se está na faixa 150-280 MB, o #2 está agravando.

---

## Fix

### Fix #1 — Substituir `add_adapter` por `get_peft_model` (igual ao Flux)

```python
# qwen_image.py — substituir L414-425

# REMOVER:
# lora_config = LoraConfig(...)
# transformer.add_adapter(lora_config)

# ADICIONAR:
from peft import LoraConfig, get_peft_model

lora_config = LoraConfig(
    r=rank,
    lora_alpha=alpha,
    init_lora_weights="gaussian",
    target_modules=["to_k", "to_q", "to_v", "to_out.0",
                    "add_k_proj", "add_q_proj", "add_v_proj"],
)
transformer = get_peft_model(transformer, lora_config)
# NÃO chamar prepare_model_for_kbit_training — quebra SDPA (mesmo motivo do Flux)
```

### Fix #2 — Adicionar validação de contagem de tensores antes de salvar

Em `lora_io.py`, após `get_peft_model_state_dict()`:

```python
lora_state_dict = _normalize_lora_keys(get_peft_model_state_dict(model))

# Validação: rank deve ser recuperável das chaves lora_A
lora_a_keys = [k for k in lora_state_dict if k.endswith("lora_A.weight")]
if not lora_a_keys:
    raise RuntimeError(
        "get_peft_model_state_dict() retornou 0 tensores lora_A — "
        "LoRA não foi injetada corretamente no modelo."
    )
# Verificar rank real vs esperado
actual_rank = lora_state_dict[lora_a_keys[0]].shape[0]
print(f"[LORA-SAVE] {len(lora_a_keys)} módulos LoRA salvos, rank efetivo={actual_rank}", flush=True)
```

Isso torna o problema visível imediatamente em vez de produzir arquivo corrompido.

### Fix #3 — Adicionar log de contagem de parâmetros treináveis pós-inject (igual ao Flux)

Flux tem (L501-502):
```python
trainable_params_count = sum(p.numel() for p in transformer.parameters() if p.requires_grad)
frozen_params_count    = sum(p.numel() for p in transformer.parameters() if not p.requires_grad)
```

Adicionar o mesmo ao Qwen logo após `get_peft_model()`:
```python
trainable = sum(p.numel() for p in transformer.parameters() if p.requires_grad)
total     = sum(p.numel() for p in transformer.parameters())
print(f"[LORA] Parâmetros treináveis: {trainable:,} / {total:,} ({100*trainable/total:.2f}%)", flush=True)
```

Para rank=128 com 7 módulos em 28 blocos: esperado ~153M parâmetros treináveis.  
Se o log mostrar << 1M, a injeção falhou.

---

## Verificação do Fix

Após aplicar Fix #1, re-treinar com rank=16 (mais rápido) e verificar:
```bash
python -c "
import safetensors.torch, sys
d = safetensors.torch.load_file(sys.argv[1])
keys = [k for k in d if 'lora_A' in k]
print(f'Módulos LoRA: {len(keys)}')
print(f'Rank efetivo: {d[keys[0]].shape[0]}')
print(f'Tamanho total: {sum(v.nbytes for v in d.values())/1024**2:.1f} MB')
" output/adapter.safetensors
```

Esperado com rank=16: `Módulos LoRA: 196`, `Rank efetivo: 16`, `~37 MB`.

---

## Spec Relacionado
- `tasks/specs/qwen-image-memory-audit.md`
- `tasks/specs/engines-auditoria-global.md` (item #7 — BitsAndBytesConfig)
