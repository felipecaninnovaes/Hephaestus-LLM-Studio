# Spec — Performance do Treino FLUX (alvo: GPU 12GB)

Origem: análise da sessão 2026-09-20 do loop `_real_train_flux`
(`engines/trainer-difusao/src/trainer_difusao/models/flux.py`) com restrição
explícita do usuário: **GPU de treino com 12GB** (não assumir folga de 16GB+).
**Status: analisado, nada implementado.** Toda linha citada foi verificada hoje.

Premissa de orçamento em 12GB: manter `quantization: 4bit` (pesos ~2,5–3GB) e
não desligar gradient checkpointing por suposição — as melhorias abaixo são as
**neutras ou positivas em VRAM**; as que consomem headroom ficam adiadas (§F).

---

## P1. DataLoader sem workers (risco zero, VRAM zero)

- `dataset.py:169-179`: `DataLoader` sem `num_workers`, `pin_memory`,
  `persistent_workers` → PIL open+resize (`dataset.py:116-131`) roda no processo
  principal; GPU ociosa esperando dados.
- **Fix:** `num_workers = int(os.environ.get("HEPH_DATALOADER_WORKERS", min(4, os.cpu_count() or 1)))`,
  `pin_memory=True`, `persistent_workers=True` (somente com workers>0), nos dois
  ramos (bucket e não-bucket). O loader do dataset de controle
  (`flux.py:910-912`) herda a mesma função.
- Determinismo preservado: ordem vem do `BucketBatchSampler` (seed própria);
  ruído/timesteps continuam gerados no processo principal.
- `collate` default lida com `prompt: str` (lista de strings) — sem custom.

## P2. Cache de latents — VAE fora do loop (maior ganho de step time)

- Hoje `vae.encode(pixel_values.float())` roda **a cada batch de cada época**
  (`flux.py:1093`), com VAE fp32 residente (`flux.py:831-833`). Dataset é
  imutável durante o job; o encode é redundante entre épocas.
- **Fix:** pré-computar **média e desvio-padrão** da distribuição do VAE (não a
  amostra) uma vez antes do loop, espelhando `TextEmbedsCache`
  (`common_pkg/text_embeds.py:13-56`): novo `LatentCache` com arquivos
  `{output}/latent_cache/{sha16}.pt`. Chave =
  `sha256(img_path + "|" + f"{w}x{h}" + "|" + quant/arch + "|" + str(target_dtype))[:16]`
  — dims do bucket entram na chave (mudança de `resolution`/bucketing invalida).
  Gravar em `target_dtype` (meta+std cabem; metade do disco vs fp32).
- No loop, substituir só a chamada do VAE:
  `x0 = mean + std * torch.randn_like(std)` — **equivalência numérica exata**
  com `DiagonalGaussianDistribution.sample()`; nenhuma aleatoriedade perdida.
  Patchify/normalização/pack (`flux.py:1095-1133`) permanecem inalterados
  (elementwise baratos).
- Pré-compute usa o próprio `dataloader` (P1 já ativo) com `torch.no_grad()`;
  itera dataset **e control_dataset**. Falha de I/O no cache → degradar para
  encode on-the-fly (mesma política try/except de `_precompute_text_cache`).
- Flag: `lora.cache_latents` default `True`; env `HEPH_LATENT_CACHE=0` desliga.
  Fase estruturada `preparing_cache` na telemetria (**alinhar com C2c de
  `docs/archive/specs/treino-observabilidade.md`** — emitir i/N, não só print).
- Custo de disco: ~1MB/imagem em 1024² (32ch→64ch packed ×2 tensores bf16);
  10k imagens ≈ 10–20GB em `/outputs` junto do cache de texto. Documentar.

## P3. Liberar encoder + VAE da VRAM após o pré-cache (cria headroom em 12GB)

- Qwen3/T5/CLIP ficam residentes a época inteira (`flux.py:478-519`,
  `760-792`, `817-819`) e o VAE idem, mesmo com `cache_text_embeddings=True`
  e P2 ativo — nenhum dos dois é usado no loop de otimização.
- **Fix:** após P2+pré-cache de texto, **se** (a) `cache_text_embeddings` e
  (b) `cache_latents` estiverem ativos: `del text_encoder_two` (T5/Qwen3);
  CLIP-T1 (`flux.py:817`) também sai se nenhum caminho on-the-fly restar.
  `gc.collect(); torch.cuda.empty_cache()`.
- Conflito com amostragem: `_generate_sample_flux` recebe encoder/VAE como args
  (`flux.py:985-1000`, `1277-1292`). **Resolução:** em vez de `del`, mover para
  CPU (`_offload_modules([...])`); o amostrador move de volta ao device antes e
  para a CPU depois (custo 1x por época de amostra, pago em VRAM todo o resto).
  Se `sample_prompt` vazio → `del` definitivo.
- VAE só sai do device com P2 ativo; com `cache_latents=False` permanece.
- Medir antes/depois com `torch.cuda.mem_get_info()` logado na fase
  `training_started` (evidência do ganho p/ a UI/telemetria).

## P4. `.item()` por micro-batch → sincronização GPU-CPU

- `flux.py:1173`: `cur_loss_raw = loss.item()` a **cada step**, forçando sync e
  esvaziando o pipeline. Guardas NaN (`1187-1188`) dependem dele.
- **Fix:** `epoch_loss` acumular como tensor no device; finiteza via
  `torch.isfinite`; `.item()` apenas nos emits (a cada 5 steps de otimização,
  `flux.py:1195-1217`, e fim de época). Comportamento externo idêntico
  (mesmos valores no `metrics.jsonl`).

## P5. Amostras de validação e allocator churn

- `samples.interval` default 1 (`flux.py:248`) + 20 steps de inferência por
  época (`sample.py:36`): em dataset pequeno o wall-clock das amostras pode
  superar o do treino; e o ciclo aloca/libera grandes tensores em placa apertada
  (fragmentação).
- **Fix:** default do intervalo → 2; aceitar `samples.steps` (default atual 20
  mantido para amostras, mas a UI pode baixar p/ 8 — campo de wire fica em F2).
- `_cleanup_cuda()` por época (`flux.py:1265`) provoca `cudaMalloc` recorrente:
  chamar somente antes/depois da amostragem e do checkpoint, não sempre.

## Ordem de execução e esforço

| # | Item | Arquivos | Esforço | Risco |
|:--|:-----|:---------|:--------|:------|
| 1 | P1 workers | `dataset.py` | ~15 LOC | baixo |
| 2 | P4 sem sync | `flux.py:1172-1188` | ~20 LOC | baixo |
| 3 | P2 LatentCache | novo em `common_pkg/` + `flux.py` | ~150 LOC | médio (invalidação) |
| 4 | P3 offload | `flux.py` + helper em `runtime.py` | ~60 LOC | médio (sampling) |
| 5 | P5 amostras | `flux.py:248`, `sample.py`, `1265` | ~25 LOC | baixo |

Tudo contido em `engines/trainer-difusao` — **nenhum contrato toca
`packages/`** na fase executável agora (flags via env/default).

## Não-metas (adiado conscientemente)

- **Grad checkpointing adaptativo/desligável** (`flux.py:860` incondicional):
  só com decisão por headroom real medido (`mem_get_info` pós-P3) — campo de
  wire `gradientCheckpointing` exige bump sequencial de `openapi.yaml` +
  `models.rs` + Forja. Fatia separada.
- **`quantization: none`** (bf16 pleno ~8GB de pesos): não cabe com batch útil
  em 12GB. Nada muda no default `"4bit"` (`flux.py:164-170`).
- **`torch.compile`**: overhead de memória do inductor/cudagraphs contra 12GB;
  reavaliar só após P1–P3 (ganho restante é menor em step mais leve).
- **TF32/bf16 no VAE**: torna-se irrelevante com P2 (VAE fora do loop).
- **SDXL/SD15**: mesmo padrão de VAE in-loop existe (`sdxl.py`, `sd15.py`);
  extrair para eles em pastilha separada após validação no FLUX.

## Verificação

- CPU/mock: `LatentCache` é classe pura → teste unitário de round-trip
  (chave inválida por mudança de `w/h`/dtype; `mean+std*eps` reproduz
  `sample()` dado o mesmo `eps`); teste de `build_dataloader` com workers>0 e
  bucketing (batches com shapes homogêneos). `uv run pytest` no engine.
- @gpu manual (12GB, `ENGINE_MOCK=0`): rodar mesmo dataset/config 2 épocas antes
  e depois; registrar no próprio arquivo: s/step médio, pico
  `max_memory_allocated`, presença de `preparing_cache`/`training_started` com
  free-VRAM. Meta declarada: ≥1,5× throughput sem regressão de VRAM pico;
  loss curve comparável (mesma seed).
