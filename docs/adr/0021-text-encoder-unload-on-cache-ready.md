# ADR-0021 — Text encoder unload on cache ready (adr-difusao-vram)

- **Status:** Aceito (2026-09-23, usuário)
- **Data:** 2026-09-23
- **Componentes:** `engines/trainer-difusao/common_pkg/text_embeds.py`, pipelines FLUX/SD15/SDXL
- **Fontes:** Discussão com usuário sobre VRAM desperdiçada com text encoders carregados após pré-compute.

## Contexto

Durante treinos LoRA com cache de text embeddings habilitado:
1. Pré-compute roda uma vez no início (epoch 0) → text encoders na GPU
2. Loop de treino usa cache hits → embeddings carregados de disco (CPU)
3. Text encoders **continuam na VRAM** durante todas as épocas, mesmo quando não precisados
4. Isso desperdiça ~2–8 GB de VRAM (dependendo do modelo)

**Exemplo prático:**
- FLUX.2 Klein (Qwen3 + T5): ~8 GB de VRAM + text encoders
- Treino com `ENABLE_TEXT_ENCODER_UNLOAD=false`: text encoders ~2-8 GB + transformer/VAE ~12-16 GB
- Treino com `ENABLE_TEXT_ENCODER_UNLOAD=true`: text encoders ~0 GB após pré-compute → ~12-16 GB de VRAM total

## Decisão

**D0 — Implementar flag de performance `ENABLE_TEXT_ENCODER_UNLOAD` (env var, default: false)**

Controla se encoders de texto são descarregados da VRAM após pré-compute do cache de embeddings:

| Opção | Comportamento | Runtime | Preservação de estado |
|-------|----------------|---------|----------------------|
| `ENABLE_TEXT_ENCODER_UNLOAD=false` (default) | Text encoder permanece na VRAM | Útil para retomada de treino (epoch_offset > 0) | Sim |
| `ENABLE_TEXT_ENCODER_UNLOAD=true` | Text encoder descarregado após pré-compute | Só válido para treinos sem retomada | Impróprio para resume |

**Notas de design:**
- Flag aplicada em `text_embeds.py` (única fonte de verdade)
- Injeção em cada pipeline (flux.py, sd15.py, sdxl.py) via `_precompute_text_cache_with_cleanup()`
- Zero impacto em jobs existentes (default: false)
- Zero overhead em modo mock (`torch.cuda.is_available()` check)

**Descartado:**
- Entrega imediata de GC automático (vs explícito `del` + `empty_cache()`)
- Ativação automática baseada em vram reservada (regra mais complicada do que flag explícita)
- Usar flag via JSON de job em vez de env var (hook de configuração mais limpo)

## Consequências

### Benefícios

1. **Economia de VRAM:** ~2–8 GB liberados após pré-compute (FLUX/SDXL) ou ~2 GB (SD15)
2. **Zero overhead de código:** pattern centralizado em `text_embeds.py` (DRY)
3. **Zero breaking change:** default `false`, nenhum job existente é afetado
4. **Zero duplicação:** limpeza padrão em `text_embeds.py` sem repetição em pipelines

### Custo

1. **Complexidade crescente:** pipelines agora dependem de env var (microtrade-off aceitável)
2. **Guia de uso:** docs futuros devem explicar flag e benefícios (vram-table.yaml também ajuda)
3. **Não aplicável a resume:** treinos que retomam épocas devem manter encoder na VRAM

## Complementos

- **`vram-table.yaml`:** entrada `feature: text_encoder_unload, default: false` (ponto único de documentação de features)
- **Migration:** sem alteração necessária — flag ativa-se apenas se `ENABLE_TEXT_ENCODER_UNLOAD=true`

## Roadmap futuro

- **Fase 2:** Se outras engines (trainer-yolo, trainer-clip) precisarem de pattern similar, mover `_cleanup_encoders()` para `engine-kit/vram.py` como generic wrapper
