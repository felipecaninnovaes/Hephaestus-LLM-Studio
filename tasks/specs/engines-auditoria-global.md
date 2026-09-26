# Auditoria Global — `engines/`

**Escopo:** engine-kit, trainer-difusao, trainer-yolo, trainer-clip  
**Data:** 2026-09-25  
**Status:** Pendente triagem e implementação

---

## Sumário Executivo

| Sev | # | Área | Descrição curta |
|-----|---|------|-----------------|
| 🔴 | 1 | trainer-difusao/serve | LoRA residual entre requests — pesos corrompem geração subsequente |
| 🔴 | 2 | trainer-difusao/serve | `torch.cuda.empty_cache()` ausente no `finally` do handler |
| 🔴 | 3 | trainer-difusao/train.py | Imports pesados (torch/diffusers) no top-level — carregados em `--help` e `cmd_health` |
| 🔴 | 4 | trainer-yolo/train.py | Modelo YOLO não desalocado pós-treino se chamado in-process |
| 🟠 | 5 | engine-kit/httpd.py | `read_json_body` sem limite de `Content-Length` — DoS por buffer |
| 🟠 | 6 | engine-kit/vram.py | `release_memory(*objects)` não deleta referências — semântica enganosa |
| 🟠 | 7 | trainer-difusao (todos os modelos) | `BitsAndBytesConfig` 4-bit duplicada em 4 trainers |
| 🟠 | 8 | trainer-difusao (todos os modelos) | Boilerplate de cfg parsing (epochs, rank, alpha…) duplicado em 4 arquivos |
| 🟠 | 9 | trainer-difusao/optimizers.py | Fallback silencioso de scheduler retorna `None` — treino roda sem scheduler |
| 🟠 | 10 | trainer-clip | Sem `torch.cuda.empty_cache()` quando idle — VRAM ocupada indefinidamente |
| 🟠 | 11 | trainer-clip | Contrato HTTP inconsistente com `serve_pkg` de trainer-difusao |
| 🟠 | 12 | trainer-difusao/qwen | `prompt_cache` em RAM pura vs disco (TextEmbedsCache) dos outros modelos |
| 🟡 | 13 | engine-kit/telemetry.py | `TelemetryEmitter.emit()` sem lock — race condition com múltiplas threads |
| 🟡 | 14 | trainer-difusao/quantization.py | `_die()` local duplica `engine_kit.runtime.die` |
| 🟡 | 15 | trainer-difusao/train.py | `cmd_health` emite JSON ad-hoc em vez de contrato unificado |
| 🟡 | 16 | trainer-difusao | `quant_cache` em disco sem cota de tamanho — crescimento ilimitado |
| 🟡 | 17 | engine-kit/artifacts.py | `prune_checkpoints` sem lock — race condition entre processos |
| 🟡 | 18 | trainer-yolo | Sem circuit breaker na API de autolabel (apenas retry linear) |

---

## Problemas Detalhados

---

### 🔴 #1 — LoRA residual corrompe geração subsequente (`generation/runner.py`)

**Sintoma:** `pipe.unload_lora_weights()` é chamado _antes_ da injeção de novas LoRAs, mas
somente quando `pipeline is not None` (reutilizado do cache). Se a request anterior falhou
no meio do batch sem descarregar, os pesos da LoRA anterior permanecem e a próxima
geração usa a LoRA errada.

**Evidência:** `serve_pkg/state.py` — `_pipeline_cache` reutiliza a instância; nenhum
`unload_lora_weights()` condicional no `finally` de `_real_generate`.

**Fix:** Adicionar ao `finally` de `_real_generate`:
```python
finally:
    try:
        if pipe is not None and hasattr(pipe, "unload_lora_weights"):
            pipe.unload_lora_weights()
    except Exception:
        pass
```

---

### 🔴 #2 — `finally` do handler não limpa VRAM após falha (`serve_pkg/handler.py`)

**Sintoma:** `_handle_generate` garante `_busy = False` e libera o lock no `finally`,
mas não chama `torch.cuda.empty_cache()` após exceção. Tensores intermediários do
forward pass ficam fragmentando a VRAM, causando OOM na próxima request com o
mesmo pipeline.

**Fix:** adicionar ao `finally` de `_handle_generate`:
```python
finally:
    state._busy = False
    state._gen_lock.release()
    if exc_occurred:
        import gc; gc.collect()
        try:
            import torch; torch.cuda.empty_cache()
        except Exception:
            pass
```

---

### 🔴 #3 — Imports pesados no top-level de `train.py` (torch/diffusers carregam em `--help`)

**Sintoma:** `trainer_difusao/train.py` importa `trainer_difusao.models` no topo, que
por sua vez importa `sd15`, `sdxl`, `flux` — puxando `torch`, `diffusers`,
`safetensors` ao instanciar o módulo. Executar `python -m trainer_difusao --help` ou
`cmd_health` consome ~2-3s e RAM do processo sem motivo.

**Fix:** lazy imports dentro de `cmd_train`:
```python
def cmd_train(cfg: dict) -> None:
    from trainer_difusao.models import get_trainer  # import aqui, não no topo
    ...
```

---

### 🔴 #4 — Modelo YOLO não desalocado pós-treino in-process (`trainer-yolo/train.py`)

**Sintoma:** `_real_train` cria o objeto YOLO e treina, mas não executa `del model`,
`gc.collect()` ou `torch.cuda.empty_cache()` ao final. Em execução via CLI o OS
desaloca ao encerrar; se chamado in-process (ex: testes, orquestrador), o modelo
permanece na VRAM até o GC decidir coleta-lo.

**Fix:**
```python
finally:
    del model
    import gc; gc.collect()
    try:
        import torch; torch.cuda.empty_cache()
    except Exception:
        pass
```

---

### 🟠 #5 — `read_json_body` sem limite de Content-Length (`engine-kit/httpd.py`)

**Sintoma:**
```python
content_length = int(self.headers.get("Content-Length", 0))
body = self.rfile.read(content_length)
```
Sem limite máximo. Um cliente malicioso (ou um proxy bugado) pode enviar
`Content-Length: 10000000000` e travar o processo alocando RAM até OOM.

**Fix:**
```python
MAX_BODY = 32 * 1024 * 1024  # 32 MB
if content_length > MAX_BODY:
    self.send_json(413, {"error": "payload_too_large"})
    return None
```

---

### 🟠 #6 — `release_memory(*objects)` tem semântica enganosa (`engine-kit/vram.py`)

**Sintoma:** A assinatura aceita `*objects` sugerindo que desalocará os objetos passados,
mas a implementação delega apenas para `cleanup_cuda()` — `gc.collect()` +
`empty_cache()`. As referências passadas não são deletadas (Python pass-by-reference-value).
Todo código que chama `release_memory(model)` acredita ter desalocado o modelo, mas a
referência do chamador ainda existe.

**Evidência:** `qwen_image.py` L262 chama `release_memory()` sem argumentos; em outros
pontos é chamada com objetos sem efeito.

**Fix (opção A — mínimo):** renomear para `flush_cuda_cache()` e deprecar `release_memory`.  
**Fix (opção B — correto):** aceitar refs e deletá-las dentro da função (não possível
em Python sem retornar `None` e instruir o chamador a reassinar). Manter a função mas
documentar que **não substitui `del`**.

---

### 🟠 #7 — `BitsAndBytesConfig` duplicada em 4 trainers

`sd15.py`, `sdxl.py`, `flux.py`, `qwen_image.py` constroem `BitsAndBytesConfig` com
os mesmos parâmetros (4bit NF4 + double quant + bfloat16) em copypaste.

**Fix:** mover para `common_pkg/core.py`:
```python
def make_bnb_config(quantization: str, compute_dtype=None):
    from transformers import BitsAndBytesConfig
    import torch
    dtype = compute_dtype or torch.bfloat16
    if quantization in ("4bit", "4bit-nf4"):
        return BitsAndBytesConfig(load_in_4bit=True, bnb_4bit_quant_type="nf4",
                                  bnb_4bit_use_double_quant=True, bnb_4bit_compute_dtype=dtype)
    if quantization in ("8bit", "8bit-bnb"):
        return BitsAndBytesConfig(load_in_8bit=True)
    return None
```

---

### 🟠 #8 — Boilerplate de `cfg` parsing duplicado em 4 trainers

`epochs`, `batch_size`, `rank`, `alpha`, `learning_rate`, `trigger_word`, `resolution`,
`optimizer`, `lr_scheduler`, `checkpoint_interval`, `grad_accum`, `seed` extraídos
identicamente em `sd15.py`, `sdxl.py`, `flux.py`, `qwen_image.py`.

**Fix:** `common_pkg/train_config.py` já existe — verificar se `TrainConfig` ou
`parse_lora_cfg()` cobre esses campos; se não, estender. Os trainers devem chamar
`cfg_parsed = parse_lora_cfg(cfg)` e desconstruir o dataclass.

---

### 🟠 #9 — Scheduler retorna `None` silenciosamente (`optimizers.py`)

```python
except Exception as e:
    print(f"[WARN] Falha ao criar scheduler: {e}. Sem scheduler.")
    return None  # treino continua sem LR decay
```

O loop de treino verifica `if lr_scheduler is not None: lr_scheduler.step()` —
mas `optimizer.step()` sem scheduler significa LR constante em toda execução.
Nenhuma métrica indica isso ao usuário além do print.

**Fix:** ou re-raise (correto), ou emitir via `TelemetryEmitter.error()` para que o
manager/API-principal marque o job com warning visível.

---

### 🟠 #10 — trainer-clip sem cleanup de VRAM em idle (`clip_backend.py`)

Modelo CLIP carregado na inicialização e mantido em VRAM indefinidamente.
Para RTX 3060 12 GB, CLIP ViT-L/14 ocupa ~1 GB parado — impacta treinos concorrentes.

**Fix:** implementar idle-unload com timer (ex: 10 min sem requests → `del _REAL; cleanup_cuda()`).
Alternativa mais simples: `torch.cuda.empty_cache()` após cada request de embedding.

---

### 🟠 #11 — Contrato HTTP inconsistente entre trainer-clip e trainer-difusao

| Campo | trainer-difusao `/health` | trainer-clip `/health` |
|-------|--------------------------|------------------------|
| Status | `{"ok": true, "engine": ..., "busy": ..., "vram_used_gb": ..., "uptime_s": ...}` | `{"status": "ok", "mode": ...}` |
| Erro | `{"ok": false, "error": ...}` | `{"error": "encode_failed: ..."}` |
| Mock flag | `"mock": true/false` | `"mode": "mock"/"real"` |

O `api-principal` que consome ambos precisa tratar dois formatos distintos.

**Fix:** alinhar `trainer-clip` ao contrato de `serve_pkg/handler.py`:
`{"ok": bool, "engine": "clip", "mock": bool, "busy": bool, "vram_used_gb": float, "uptime_s": float}`.

---

### 🟠 #12 — Qwen usa `prompt_cache` em RAM; outros modelos usam `TextEmbedsCache` em disco

**Divergência arquitetural:**
- `sd15`, `sdxl`, `flux`: `TextEmbedsCache` serializa embeddings em disco com hash SHA256,
  sobrevivendo a reinícios e sendo compartilhados entre runs.
- `qwen_image`: `prompt_cache: dict` em RAM — perdido ao final do treino, recalculado
  a cada run, e cresce ilimitadamente na sessão se o dataset for grande.

**Fix:** migrar `qwen_image.py` para `TextEmbedsCache` via `common_pkg/text_embeds.py`,
alinhando ao padrão dos outros modelos.

---

### 🟡 #13 — `TelemetryEmitter.emit()` sem lock (`engine-kit/telemetry.py`)

`emit()` faz append em `telemetry.jsonl` e modifica `self._current_phase` e
`self._last_progress` sem `threading.Lock`. Em trainer-difusao, se geração e treino
rodarem threads concorrentes (ou callbacks de progresso forem emitidos de thread worker),
o arquivo pode receber linhas JSON intercaladas (corrompendo o JSONL).

**Fix:** adicionar `self._lock = threading.Lock()` no `__init__` e envolver `emit()`:
```python
with self._lock:
    # append ao arquivo + atualizar campos
```

---

### 🟡 #14 — `_die()` local em `quantization.py` duplica `engine_kit.runtime.die`

```python
# quantization.py
def _die(msg: str) -> None:
    print(msg, file=sys.stderr)
    sys.exit(1)
```

`engine_kit.runtime.die` já faz exatamente isso. Remover e importar do kit.

---

### 🟡 #15 — `cmd_health` emite JSON ad-hoc (`trainer-difusao/train.py`)

```python
print(json.dumps({"status": "ok", "engine": "diffusion", ...}))
```

Não usa `TelemetryEmitter` nem o contrato de `serve_pkg`. Se o contrato mudar,
este print não é atualizado automaticamente.

---

### 🟡 #16 — `quant_cache` em disco sem cota de tamanho

`loaders/quant_cache.py` e `loaders/transformer_loader.py` salvam modelos quantizados
em `~/.cache/hephaestus/quantized/` (ou `/data/outputs/.cache/quantized`) sem TTL
nem limite. Em servidores com muitos modelos diferentes, o cache pode consumir
centenas de GB sem cleanup automático.

`TEXT_ENCODER_CACHE_MAX_GB` (default 48 GB) existe para merge cache — aplicar
o mesmo mecanismo LRU ao `quant_cache`.

---

### 🟡 #17 — `prune_checkpoints` sem lock entre processos (`engine-kit/artifacts.py`)

Se dois jobs de treino do mesmo modelo rodarem em paralelo (ex: restart com
`epoch_offset`), ambos podem listar e deletar checkpoints simultaneamente,
causando race condition. O `p.unlink()` captura `OSError` mas a listagem anterior
já pode ter sido calculada com estado inconsistente.

**Mitigação:** usar `fcntl.flock` no arquivo `checkpoints_dir/.prune.lock` antes
de listar.

---

### 🟡 #18 — Autolabel sem circuit breaker (`trainer-yolo/autolabel_pkg/vision_api.py`)

Retry linear de até 3 tentativas com `time.sleep(1.5 * (attempt+1))` para erros
HTTP 429/5xx e `time.sleep(1.0)` para `URLError`. Sem circuit breaker: se a API de
visão estiver down, cada imagem do dataset bloqueia por ~5s antes de falhar, tornando
o autolabel de datasets grandes extremamente lento.

**Fix:** implementar circuit breaker simples com contagem de falhas consecutivas
(threshold=5) → `raise` imediato sem tentar.

---

## Mapa de Arquivos Afetados

```
engines/
├── engine-kit/src/engine_kit/
│   ├── httpd.py          → #5 (limit Content-Length)
│   ├── vram.py           → #6 (semântica release_memory)
│   ├── telemetry.py      → #13 (lock thread-safety)
│   ├── artifacts.py      → #17 (lock prune_checkpoints)
│   └── runtime.py        → referência para #14
│
├── trainer-difusao/src/trainer_difusao/
│   ├── train.py          → #3 (lazy imports), #15 (cmd_health)
│   ├── optimizers.py     → #9 (scheduler None silencioso)
│   ├── quantization.py   → #14 (_die duplicado)
│   ├── models/
│   │   ├── sd15.py       → #7, #8
│   │   ├── sdxl.py       → #7, #8
│   │   ├── flux.py       → #7, #8
│   │   └── qwen_image.py → #7, #8, #12 (+ achados em spec qwen-image-memory-audit.md)
│   ├── loaders/
│   │   └── quant_cache.py → #16
│   ├── generation/
│   │   └── runner.py     → #1 (LoRA residual)
│   └── serve_pkg/
│       └── handler.py    → #2 (VRAM no finally)
│
├── trainer-yolo/src/trainer_yolo/
│   ├── train.py          → #4 (cleanup pós-treino)
│   └── autolabel_pkg/vision_api.py → #18
│
└── trainer-clip/src/trainer_clip/
    ├── clip_backend.py   → #10 (idle VRAM)
    └── serve.py / server.py → #11 (contrato HTTP)
```

---

## Critérios de Aceitação por Prioridade

### Fase 1 — Crítico (impacto imediato em correctness/OOM)
- [ ] `unload_lora_weights()` no `finally` de `_real_generate` (#1)
- [ ] `torch.cuda.empty_cache()` no `finally` de `_handle_generate` após exceção (#2)
- [ ] Imports de `torch`/`diffusers` lazy em `train.py` (#3)
- [ ] `del model` + `cleanup_cuda()` no `finally` de `trainer_yolo/_real_train` (#4)

### Fase 2 — Alto (consistência e robustez)
- [ ] `read_json_body` com limite 32 MB (#5)
- [ ] `make_bnb_config()` centralizado em `common_pkg/core.py`, removido dos 4 trainers (#7)
- [ ] `parse_lora_cfg()` em `common_pkg/train_config.py` cobrindo todos os campos comuns (#8)
- [ ] Scheduler fallback emite via `TelemetryEmitter.error()` e re-raise (#9)
- [ ] Contrato `/health` de `trainer-clip` alinhado ao padrão `serve_pkg` (#11)
- [ ] `qwen_image.py` migrado para `TextEmbedsCache` (#12)

### Fase 3 — Menor (qualidade e manutenção)
- [ ] `TelemetryEmitter.emit()` com `threading.Lock` (#13)
- [ ] `quantization.py` usa `engine_kit.runtime.die` (#14)
- [ ] `cmd_health` usa contrato unificado (#15)
- [ ] `quant_cache` com LRU por tamanho (#16)
- [ ] `prune_checkpoints` com `flock` (#17)
- [ ] Circuit breaker em `vision_api.py` (#18)
- [ ] `release_memory` documentada ou renomeada (#6)

---

## Spec Relacionado
- `tasks/specs/qwen-image-memory-audit.md` — achados específicos do pipeline qwen_image.py (7 itens adicionais)
