# ADR-0026 — FLUX.2 Klein 9B (base) como arquitetura `flux-2-klein-9b`

Data: 2026-10-06 · Status: ACEITA (usuário) · Fatias: backend `6e1715a`
(OpenAPI 0.31.0, migration 0025), fix de geração `ea2ed14`, web `f13dfd1`,
engine `feat/flux2-klein-9b-engine` (`0625d3c`)

## Contexto

Treino/geração Flux.2 só existia no Klein 4B. O 9B base
(`black-forest-labs/FLUX.2-klein-base-9B`) muda as dimensões do transformer:
4B hidden 3072 (24 heads×128), `joint_attention_dim` 7680; 9B hidden 4096
(32 heads×128), `joint_attention_dim` 12288, 8 double + 24 single blocks — LoRA
e checkpoint de uma variante não carregam na outra.

Spike RTX 3060 12 GB (2026-10-06):
- Treino 4-bit, batch 1, 768/1024 px, rank 16/32: OK **só com text encoder
  unload**; pico 11,86–11,90 GiB (precompute de embeds), loop 7,4–9,0 GiB;
  2,0× o tempo/step do 4B. Flags default ⇒ OOM.
- Geração 4-bit real, 1024 px/28 steps: pico 11,1 GiB, 7,3 s/it.

O spike também expôs que o campo `quantization` da geração Klein era no-op
(4B rodava bf16+offload; 9B dava OOM) — corrigido em `ea2ed14` antes da fatia
do engine (ver PITFALLS, seção Engines).

## Decisões

- **D1 — Só o base.** Novo arch `flux-2-klein-9b` em todo enum que lista
  `flux-2-klein-4b` (`baseModel` de treino/geração, `Model.arch`, hint de
  upload); `models_arch_check` expandido pela migration 0025. Alias legado
  `flux` continua = 4B. `distilled=true` + 9B ⇒ 400 `invalid_request`.
- **D2 — 4-bit como alvo.** VRAM mínima 9B (treino e geração), sem
  headroom (o manager soma +2 GB ⇒ 12 GB exigidos em 4bit): 10 GB
  (2/4bit), 18 GB (6/8bit), 26 GB (none); picos reais 11,9 GiB (treino) e
  10,4 GiB (geração); linhas `flux-2-klein-9b` em
  `packages/policies/vram-table.yaml` com as notas do spike.
- **D3 — Unload do text encoder forçado no 9B.** O engine liga cache de
  embeds + unload do encoder no 9B independentemente de
  `ENABLE_TEXT_ENCODER_UNLOAD` (ADR-0021); sem isso o treino não cabe em 12 GB.
  Repo via `FLUX_9B_MODEL_ID` (default o repo base 9B); metadata do adapter
  `base_model=flux-2-klein-9b`.
- **D4 — Guarda LoRA×arch.** No manager (`jobs/resolve.rs::resolve_loras`),
  LoRA com `arch` não nulo ≠ arch efetivo da geração (base ou custom,
  normalizado) ⇒ 400 nomeando o modelo; `arch` NULL (legado) passa. O sniff
  de upload (`models/validate.rs`) separa 4B/9B por `__metadata__.base_model`
  e depois por shape; indeterminável ⇒ 4B (`variant_assumed`).
- **D5 — Licença.** O FLUX.2 Klein 9B é distribuído sob licença
  não-comercial da Black Forest Labs; uso comercial dos pesos/saídas exige
  verificação da licença pelo operador. O Studio não embute os pesos.

## Fora de escopo (backlog §1)

- Variante destilada 9B (licença gated não aceita; +~35 GB de disco no nó).
- `textEncoderModelId` com 9B (segue só 4B ⇒ 400).
- `customModelId` 9B (registro/sniff classificam, submit ⇒ 400
  `unsupported_architecture`).
- Quantização ignorada na geração base SDXL/SD15, fallback silencioso de
  bitsandbytes e LoRA salva sem `alpha` (bugs de follow-up).

## Consequências

- Contrato público minor (`openapi 0.31.0`); web expõe "FLUX.2 Klein 9B
  (base)" com destilado/encoder custom desabilitados e seletor de LoRA
  filtrado por arch.
- Daemon de geração mantém um pipeline por vez; troca 9B↔4B recarrega
  (spec/cache key já carregam `base_model`).
