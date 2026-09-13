# ADR-0018 — Treino de Difusão LoRA: empacotamento com captions, job de treino e engine trainer-difusao (Fatia Difusão)

- **Status:** ACEITA (2026-09-12). Especificação executável da fatia.
- **Data:** 2026-09-12
- **Componentes:**
  - `packages/contracts`: spec OpenAPI bumped de 0.16.0 para 0.17.0 com rota `POST /api/jobs/diffusion` e `PackageRequest.engine` admitindo `'yolo' | 'diffusion'`.
  - `services/api-principal`: suporte a `engine='diffusion'` no empacotamento de datasets (`build_package`), gerando pares de imagens e arquivos de legenda `.txt`; rota de submit `POST /api/jobs/diffusion` (202) com validação de hiperparâmetros LoRA.
  - `services/manager`: aceita `kind='diffusion_train'` com `engine='diffusion'`, atribui requisito de VRAM baseado no modelo base (8/12/16 GB), roteia com `orchestrator_hint` e registra checkpoint `adapter.safetensors` via hook de job `done`.
  - `services/orchestrator`: matriz `(engine, mode)` com `("diffusion", "train")`, mapeamento de volumes e coleta de artefatos `adapter.safetensors` (`kind='model'`) e `metrics.jsonl` (`kind='metrics'`).
  - `engines/trainer-difusao`: runner Python com comando `train --config <path>` que sob `ENGINE_MOCK=1` simula épocas, streama métricas de loss para `metrics.jsonl` e gera o peso `adapter.safetensors`.
  - `apps/web`: tela **Forja Difusão** (`/difusao`) ativa na Sidebar (badge Roadmap removido), seletor de dataset com indicador de legendas (`captionsCount`), configuração de hiperparâmetros LoRA (baseModel, triggerWord, epochs, lr, rank, alpha) e integração com `NodeSelect`.
- **Fontes:**
  - `IDEIA.md` §1/:9 (abas separadas por tipo de modelo/tarefa), §2/:30-36 (Difusão: Flux, SDXL, SD1.5 com preparo via AutoLabel; abas de treino próprias).
  - `docs/backend.md` §9 (endpoints de jobs e models), §10 (schema `models`, `jobs`, `captions`).
  - `docs/frontend.md` §4.3 (Forja e Engenharia: `/difusao`).
  - `docs/adr/0012-models-real.md` (suporte a artefato model gerando row canônica em `models`).
  - `docs/adr/0016-autolabel-v1.md` (captions geradas para difusão).

---

## 1. Contexto e Problema

O Hephaestus LLM Studio já possui o ciclo de detecção (YOLO) completo (anotação de boxes, AutoTracker, treino com checkpoints e playground) e preparo de legendas via AutoLabel.
No entanto, a categoria **Difusão** — núcleo central do `IDEIA.md` §2 para ajuste fino e treinamento de adaptadores LoRA sobre modelos geradores como SDXL, SD 1.5 e Flux — ainda estava pendente no backend e marcada como "Roadmap" no frontend.

Com a conclusão recente da **Gestão de Modelos** (suporte canônico a `engine='diffusion'` e formato `.safetensors`), as fundações de dados e armazenamento estão prontas. Esta fatia implementa a esteira vertical de ponta a ponta para treino de Difusão LoRA.

---

## 2. Decisões Numeradas

### D0 — Empacotamento de Datasets para Difusão (`engine='diffusion'`)

- O empacotamento em `POST /api/datasets/:id/package` passa a aceitar `engine="diffusion"`.
- A função `build_package` (compartilhada entre a rota e a submissão de jobs) gera uma árvore de materialização adaptada para treino de difusão:
  1. Consulta as imagens ativas do dataset (`deleted_at IS NULL`).
  2. Consulta a tabela `captions` correspondente a cada imagem.
  3. Para cada imagem, materializa no diretório temporário:
     - `{stem}.webp` (ou formato de imagem normalizado).
     - `{stem}.txt`: arquivo de texto contendo a legenda da imagem. Se o job especificar um `trigger_word`, a legenda é opcionalmente prefixada por ele (ex.: `"<trigger> <caption_text>"`). Se a imagem não tiver legenda, o arquivo `.txt` conterá o `trigger_word` (ou estará vazio).
  4. Gera `dataset.yaml` / `metadata.jsonl` com índice dos pares `{"file_name": "...", "caption": "..."}`.
  5. Compacta em `dataset.zip` e persiste em `packages/<version_id>/dataset.zip` no bucket S3 via `StoragePort`.

### D1 — Endpoint de Submissão de Job de Difusão (`POST /api/jobs/diffusion`)

- Rota protegida no `api-principal`: `POST /api/jobs/diffusion` → 202 Accepted.
- Body (`DiffusionJobRequest`):
  ```json
  {
    "datasetId": "uuid",
    "baseModel": "sdxl | flux | sd15",
    "triggerWord": "minha_classe (opcional)",
    "epochs": 10,
    "batchSize": 1,
    "learningRate": 0.0001,
    "rank": 16,
    "alpha": 16,
    "weights": "uuid opcional de checkpoint de models",
    "orchestratorId": "uuid opcional do nó"
  }
  ```
- Validações:
  - `datasetId` deve ser UUID válido e o dataset deve conter ao menos 1 imagem ativa (senão 409 `dataset_not_ready`).
  - `baseModel` deve pertencer ao conjunto aceito (`sdxl`, `flux`, `sd15`).
  - `epochs` entre 1 e 100.
  - `rank` entre 4 e 128.
  - `learningRate` entre `1e-6` e `0.01`.
  - `weights` (se fornecido) deve pertencer à engine `diffusion`.
- O handler empacota o dataset com `engine="diffusion"`, serializa os parâmetros em `config.yaml` e envia `create_job` ao manager.

### D2 — Manager: Kind `diffusion_train` e Políticas de VRAM

- O Manager aceita jobs com `kind="diffusion_train"` e `engine="diffusion"`.
- VRAM mínima calculada com base no modelo base:
  - `sd15`: 8 GB VRAM.
  - `sdxl`: 12 GB VRAM.
  - `flux`: 16 GB VRAM.
- Despacho prioriza nós online com capacidade de VRAM declarada no heartbeat.
- O hook do manager no `report_job` com status `done` detecta o artefato de modelo emitido pelo engine (`adapter.safetensors`) e cria automaticamente o registro na tabela `models` com `engine='diffusion'`, `source='train'` e `name='<job_id>_lora.safetensors'`.

### D3 — Engine Python `trainer-difusao` (Mock Determinístico)

- O módulo `trainer_difusao` aceita execução via CLI:
  `python -m trainer_difusao train --config config.yaml`
- Sob `ENGINE_MOCK=1`:
  - Carrega a configuração (`epochs`, `learningRate`, etc.).
  - Simula as épocas com escrita progressiva em `metrics.jsonl`:
    `{"epoch": 1, "step": 10, "loss": 0.145}`
  - No término, gera um arquivo válido `adapter.safetensors` em `outputs/<job_id>/adapter.safetensors` com magic header e metadados mínimos de safetensors.

### D4 — Interface Web (`/difusao`) e UX

- Rota criada em `apps/web/app/(studio)/difusao/page.tsx` utilizando componente `ForjaDifusaoSetup.tsx`.
- Form Vidro Óptico / Dark-Only:
  - Seletor de Dataset mostrando total de imagens e indicador de quantas possuem legendas via AutoLabel.
  - Cards de configuração com presets rápidos para SDXL, Flux e SD1.5.
  - Campos para Trigger Word, Épocas, Learning Rate e Rank LoRA.
  - Seletor de Nó (`NodeSelect`).
- Sidebar atualizada: link `/difusao` passa a `isAvailable: true`, sem badge "Roadmap".
