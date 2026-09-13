# ADR-0020 — Playground de Difusão: geração Text-to-Image com LoRA e quantização

- **Status:** ACEITA
- **Data:** 2026-09-13
- **Componentes:** `packages/contracts` (OpenAPI 0.19.0 → 0.20.0), `services/api-principal` (BFF: `POST /api/jobs/diffusion/generate`, validação + `generate_diffusion_generate_config_yaml`), `services/manager` (job `engine='diffusion'`, `mode='generate'`, `kind='diffusion_generate'`, staging de `weights_ref`), `services/orchestrator` (matriz `(diffusion, generate)` despacha subcomando `generate` e coleta artefato `generated.png`), `engines/trainer-difusao` (subcomando `generate` com mock determinístico e pipelines reais FLUX.2 Klein 4B, SDXL e SD 1.5), `apps/web` (página `/playground` com abas de modo Difusão vs YOLO, painel de controle Dark-Only, canvas de visualização e histórico de sessão).

## Contexto

Após a consolidação do treinamento LoRA de difusão (ADR-0018) com quantização configurável (4-bit NF4, 8-bit BNB e FP16) e telemetria de épocas, o estúdio necessita de um ambiente interativo (Playground) para que os usuários possam testar os modelos de difusão base e seus adaptadores LoRA treinados ou importados.

O Hephaestus opera em nós locais ou remotos com GPU (como a RTX 3060 12GB no TrueNAS). Por conseguinte, a geração de imagens de difusão é executada como um job assíncrono durável na fila (`POST /api/jobs/diffusion/generate`), aproveitando a esteira comprovada de despacho, monitoramento de nós e coleta de artefatos S3.

## Decisões

### D0 — Rota e Transporte
- `POST /api/jobs/diffusion/generate` (202 Accepted, retorna `SubmitJobResponse` com `jobId`).
- Job: `engine: "diffusion"`, `mode: "generate"`, `kind: "diffusion_generate"`.
- O payload aceita `baseModel`, `prompt`, `negativePrompt` (opcional), `width`, `height`, `steps`, `guidanceScale`, `seed`, `quantization`, `weights` (UUID opcional de modelo LoRA na tabela `models`), `loraScale` e `orchestratorId`.

### D1 — Engine Python (`trainer-difusao`)
- Novo subcomando `python -m trainer_difusao generate --config <config.yaml> --output <output_dir>`.
- Em modo `ENGINE_MOCK=1`: Geração sintética imediata e determinística em Pillow com metadados e desenho vetorial representativo.
- Em modo `ENGINE_MOCK=0`: Execução de pipeline Diffusers real (`FluxPipeline`, `StableDiffusionXLPipeline`, `StableDiffusionPipeline`) com quantização selecionada e injeção do LoRA via `load_lora_weights` quando configurado.
- Imagem gerada salva em `outputs/generated.png`.

### D2 — Orquestrador e Manager
- O Manager valida `weights_id` (verificando se o modelo pertence à engine `diffusion`) e injeta `weights_ref` no `params`.
- O Orquestrador mapeia `("diffusion", "generate")` para o subcomando `generate` e faz o upload de `outputs/generated.png` para o S3 sob `artifacts/{job_id}/generated.png` com `kind: "generated"`.

### D3 — Frontend Web (`/playground`)
- Alternador de abas no topo: `Geração (Difusão)` e `Detecção (YOLO)`.
- A aba YOLO é mantida 100% funcional e inalterada.
- A aba de Difusão provê controles ergonômicos no design system Dark-Only Vidro Óptico, visualizador em alta definição com zoom e galeria de histórico de sessão com atalhos para re-geração.
