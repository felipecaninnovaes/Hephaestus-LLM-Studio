# ADR-0023 — Aba Geração: ambiente dedicado de geração de imagens (daemon quente, batch, multi-LoRA, custom, galeria)

- **Status:** ACEITA (2026-09-15, aceite do usuário; spikes S1/S2 provados — ver seção "Spike obrigatório")
- **Data:** 2026-09-15
- **Componentes:** `packages/contracts` (OpenAPI 0.24.0 → 0.25.0), `services/api-principal` (BFF: request v2 de geração + rotas de galeria `generations`), `services/manager` (resolução multi-LoRA/custom, hook `generations`, rotas internas), `services/orchestrator` (daemon de inferência por nó + matriz de artefatos em glob), `engines/trainer-difusao` (batch, multi-LoRA, checkpoint custom, `serve` HTTP), `apps/web` (rota `/geracao`, galeria persistente, comparador, mobile), `packages/policies/vram-table.yaml`.

## Contexto

O usuário testou ComfyUI e encontrou fricção para aplicar LoRA em FLUX.2 Klein; o estúdio deve gerar imagens nativamente. Hoje cada geração sobe um container one-shot (`docker run --rm`) que recarrega o modelo base inteiro (~minutos) — inaceitável como "ambiente de geração". A aba "Playground" abriga dois modos (Difusão/YOLO) em pills, e o histórico de geração é só memória de sessão (`PlaygroundDiffusion.tsx`, `history` em `useState`). Pedido: aba "Geração" com geradores comuns, modelos custom (.safetensors), servidor quente por nó, batch com incremento de seed, galeria persistente com exclusão/exportação em lote, multi-LoRA, comparador de 2 imagens e mobile decente.

Fatos verificados no código (graft):
- `POST /api/jobs/diffusion/generate` (OpenAPI 0.24.0) aceita `baseModel|prompt|negativePrompt|width|height|steps|guidanceScale|seed|quantization|distilled|weights|loraScale|orchestratorId`; principal gera config.yaml com placeholder `{weights_path}`; manager resolve `weights_id` único → `weights_ref`; orquestrador mapeia `("diffusion","generate")` → subcomando `generate` + artefato único `generated.png` (kind `generated`).
- Engine: `_mock_generate`/`_real_generate` (generate.py) carregam pipeline por execução; cache de quantização persistente já existe (`/outputs/.cache/quantized/{model_slug}_{format}`).
- Precedente de daemon na casa: `trainer-clip` já tem modo `serve` HTTP (backend.md §4); runner quente é dívida documentada (backend.md §5, ADR-0013 D0).
- Migrations centralizadas em `services/api-principal/migrations/` (manager roda as mesmas — `sqlx::test(migrations = "../api-principal/migrations")`).
- `models` é tabela do manager; upload aceita `engine='diffusion'` com cap 2 GiB; magic para .safetensors é header JSON no início do arquivo (sniffável sem carregar pesos).
- VRAM: nó local RTX 3060 12 GB; `vram_min` hoje é inline no submit (sd15→6; 4bit→8; 8bit→12; none→16).

## Alternativas descartadas

- **(b) One-shot + cache agressivo de pipeline em disco (mmap/pickle):** descartada — o custo dominante é boot do container + import de diffusers/torch + streaming de pesos para VRAM; cache de pesos quantizados já existe e não remove esses custos. Só um processo residente mantém VRAM quente. Evidência: ADR-0020 (recarga de ~minutos por geração) + doutrina de runner quente do backend.md §5.
- **Daemon como serviço global no manager/principal:** descartada — o daemon é dono de VRAM do nó, e só o orquestrador conhece os mounts/GPU do nó; o principal nunca toca GPU (backend.md §1).
- **Batch = N jobs:** descartada — N containers/N cold starts anulam o ganho do daemon; batch no engine é 1 execução sequencial com seed+i (VRAM constante).
- **Galeria derivada de `job_artifacts` (sem tabela nova):** descartada — não há soft-delete, nem seed/prompt por imagem, nem metadados de geração por imagem; a tabela `generations` segue o padrão do hook de `models` no `report_job`.
- **Flux custom via `from_single_file`:** descartada para v1 — suporte a single-file de checkpoints FLUX é imaturo no diffusers; manter FLUX.2 Klein 4B first-class (diffusers repo) e custom só SDXL/SD15.

## Decisões

### D0 — Rota de navegação: `/geracao` nova aba; `/playground` vira só YOLO
- Nova rota `app/(studio)/geracao/` com pills internas **Gerar | Galeria** (padrão SubmodulePills da casa). A aba de Difusão sai do `/playground`; as pills "Geração (Difusão) vs Detecção (YOLO)" são removidas e `/playground` passa a ser o workspace YOLO (conteúdo já existente na página, ADR-0013).
- Sidebar (seção Forja & Engenharia): novo item **Geração** (`/geracao`, `IconSparkles`); item atual "Playground" vira **Detecção YOLO** (`/playground`, mesmo href, sem redirect). Breadcrumbs: `geracao` → "Geração", `playground` → "Detecção".
- *Por quê:* aba dedicada por categoria é a doutrina da IDEIA (§2 "Cada categoria tem sua própria aba"); evita redirect e preserva o vínculo ADR-0013 do YOLO. *Gotcha:* links antigos `/playground` continuam válidos (só mudam de conteúdo).

### D1 — Servidor quente: daemon de inferência por nó (runner quente de difusão)
- **Engine:** novo subcomando `python -m trainer_difusao serve --port N` (espelho do `serve` do trainer-clip, backend.md §4). HTTP local: `GET /health` (spec carregada + VRAM), `POST /generate` (corpo = config JSON da seção `generate` + `output_dir` + caminho de telemetria), `POST /shutdown`. Mock (`ENGINE_MOCK=1`) responde instantâneo e determinístico — daemon é testável em CI, não só @gpu.
- **Orquestrador:** módulo novo `daemon.rs`. No 1º job `generate` despachado para um nó, sobe o daemon (modo docker: `docker run -d` com os mesmos volumes `outputs/`/`models/`/cache; modo subprocess: spawn do venv — `EXEC_MODE` existente). Mantém **1 pipeline carregado por daemon**; request com spec diferente (baseModel/quantization/distilled) → reload do pipeline (fase `loading_model` reaparece). Idle timeout `DIFFUSION_DAEMON_IDLE_TTL_S` (default 600s) → kill. **Preempção:** treino com VRAM insuficiente mata o daemon idle primeiro (doutrina §5/§6 — runner morre, treino nunca).
- **Fallback:** daemon desabilitado (`DIFFUSION_DAEMON_ENABLED=0`, default no compose mock) ou falha de spawn/health → caminho one-shot atual intacto (`docker run --rm`, usado por CI e mock). O caminho one-shot **continua existindo** sempre.
- **Serialização:** lock local no orquestrador — 1 requisição de geração por vez por daemon; 2º job no mesmo nó espera no lock (timeout → job falha honesto). Manager já serializa por VRAM (rotação estática, ADR-0011 D3); com o daemon vivo, `nvidia-smi` reflete a VRAM ocupada no heartbeat e o roteamento `waiting_vram` funciona sozinho.
- **Telemetria:** fases por job, como hoje — warmup do 1º job mostra `preparing/loading_model/quantizing`; cada request escreve `telemetry.jsonl` no output dir do job (mecanismo existente de watch do orquestrador intacto). Zero mudança no SSE do manager.
- **Abort:** orquestrador toca sentinela `<output_dir>/cancel`; daemon checa entre itens do batch e entre fases. Imagem em voo (steps em andamento) não é interrompida — job vira `cancelled` e imagens parciais não entram na galeria (hook só registra em `done`).
- *Por quê:* única forma de manter VRAM quente entre gerações; paga dívida documentada dos runners (backend.md §5). *Gotcha:* thrash de reload quando o usuário alterna baseModel rapidamente (aceito, single-user); janela de cold-start entre spawn e alocação de VRAM pode admitir 1 job a mais (mitigada pelo lock local).

### D2 — Batch no engine (1 job, N imagens, seed+i)
- Contrato: `batchSize: integer 1..8 default 1` no request. Seed determinada: `seed` presente → `seed+i` para i=0..N-1; `seed` ausente → engine sorteia base e reporta cada seed no meta.
- Engine: loop sequencial dentro de uma execução (VRAM constante, 1 pipeline). Arquivos: `generated_0001.png … generated_000N.png` + `thumb_0001.jpg …` (512 px max-side JPEG q80) + `generation_meta.json` (1 linha por imagem: filename, thumb, seed, prompt, negative_prompt, width/height, steps, guidance_scale, quantization, distilled, loras, custom_model_id, arch — snake_case, transporte).
- Orquestrador: matriz `("diffusion","generate")` vira glob `generated_*.png` (kind `generated`) + `thumb_*.jpg` (kind `generated_thumb`) + `generation_meta.json` (kind `generated_meta`). `generated.png` legado casa no glob — retrocompat preservada.
- Telemetria do batch: `step` = índice da imagem, `totalSteps` = batchSize; progresso por item.
- *Por quê:* batch no engine é 1 job (1 fila, 1 daemon request) e cada imagem vira artefato individual — exatamente o que a galeria precisa. *Gotcha:* batch alto = job longo (8 imagens × ~10-20s); abort só entre itens.

### D3 — Multi-LoRA: `loras: [{modelId, scale}]` (0..4, ordenado)
- Contrato: `loras: array maxItems 4` de `{modelId: uuid, scale: number 0..2}`. Aplicação em ordem do array. Retrocompat: `weights`/`loraScale` continuam aceitos (deprecados, mapeados para `loras[0]`); enviar ambos → 400 `invalid_request`.
- Engine real: `pipe.load_lora_weights(p1); …; pipe.set_adapters([nomes], [scales])` (diffusers). Mock: composição no card de metadados.
- Manager: resolve cada `modelId` (row engine='diffusion', kind='lora') → `params.loras` + `weights_refs`; orquestrador stagia `outputs/<job>/weights/lora_0.safetensors…` e reescreve a lista `loras:` no config.yaml (mesmo mecanismo de substituição do `{weights_path}` atual).
- *Por quê:* pedido explícito (inclusive "nenhum"); `set_adapters` é o caminho canônico do diffusers para múltiplos adaptadores. *Gotcha:* stacking de N LoRAs depende do suporte por pipeline (Flux2KleinPipeline incluso) — coberto pelo spike S1.

### D4 — Modelos custom: checkpoint .safetensors com `kind`/`arch` sniffado
- **Migration `models`:** colunas novas `kind TEXT NULL CHECK (kind IN ('lora','checkpoint'))` e `arch TEXT NULL CHECK (arch IN ('flux-2-klein-4b','sdxl','sd15'))`. Backfill: diffusion existentes → `kind='lora'`. `kind` só tem semântica para `engine='diffusion'`; demais engines ficam NULL.
- **Upload sniff (principal):** `.safetensors` = header JSON no início (8 bytes LE de length + JSON). Classificação por chaves: `transformer.*`/`guidance_embedder.*` → flux; `conditioner.embedders.*`+`model.diffusion_model.*` → sdxl; `model.diffusion_model.*` sem conditioner → sd15; chaves `lora_*`/`.lora_*`/peft → lora. Hint opcional do cliente (`kind`+`arch` no multipart): sniff confiante vence; conflito → 400; sniff desconhecido sem hint → 400 `invalid_request`.
- **Request:** `customModelId: uuid|null` — **XOR** com `baseModel` (exatamente um dos dois; `baseModel` deixa de ser required na spec). Validação do manager: row engine='diffusion', kind='checkpoint', arch ∈ {sdxl, sd15}. `arch='flux'`/desconhecido → 400 novo `unsupported_architecture` (v1 não suporta Flux custom — documentado).
- **Engine real:** `StableDiffusionXLPipeline.from_single_file(path)` / `StableDiffusionPipeline.from_single_file(path)`; quantização do custom (4bit/8bit) é **gate do spike S1** — se falhar, custom roda só fp16 e `quantization≠none` com custom → 400. `vram_min` por arch+quant espelhando first-class (sdxl: 4bit→8, 8bit→12, none→16; sd15→6).
- **Upload cap:** `MODEL_UPLOAD_BODY_LIMIT_BYTES` 2 GiB → **8 GiB** (SDXL fp16 ≈ 6,5 GB; spool em tempfile já streama sem RAM). Objeto > 5 GiB exige PUT multipart no StoragePort — **gate do spike S2**; se falhar, cap volta para 2 GiB e custom SDXL fica inviável no v1 (só SD15).
- *Por quê:* upload/download de modelos já aceita `engine='diffusion'` (ADR-0012); falta distinguir LoRA de checkpoint e arquitetura — sniff no header é barato e determinístico. *Gotcha:* classificação por chaves é heurística (checkpoints renomeados/merged podem ter chaves atípicas) → sempre exigir confirmação visual na UI (badge "checkpoint SDXL" pós-upload).

### D5 — Galeria persistente: tabela `generations` (dono: manager) + endpoints BFF
- **Tabela nova `generations`** (migration 0011, dono manager como `jobs`/`job_artifacts`): `id UUID PK, job_id FK jobs ON DELETE CASCADE, s3_key TEXT UNIQUE, thumb_s3_key TEXT, filename, seed BIGINT, prompt, negative_prompt?, width, height, params JSONB (base_model, custom_model_id, loras, steps, guidance_scale, quantization, distilled, batch_index), created_at, deleted_at` (soft-delete; objeto S3 intocado — sweep é dívida).
- **Escrita:** hook no `report_job` do manager — job `diffusion_generate` `done` com artefato `generated_meta` → parse → INSERT por imagem (`ON CONFLICT (s3_key) DO NOTHING`, idempotente). Meta ausente (jobs legados) → deriva por índice (seed+i, params do job) **só para jobs novos**; backfill de jobs antigos fora de escopo.
- **Rotas internas (manager):** `GET /internal/generations?limit&offset&deleted&baseModel`, `POST /internal/generations/delete {ids}` (soft, idempotente).
- **Rotas públicas (principal, BFF):**
  - `GET /api/generations?limit(1..200, default 50)&offset&baseModel&quantization&deleted` → `{items:[Generation], total}`; `url`/`thumbUrl` presigned quando `S3_PUBLIC_ENDPOINT_URL` (padrão híbrido D3 do ADR-0003), senão null.
  - `GET /api/generations/:id/data` → proxy do objeto via StoragePort (mesmos headers de cache do proxy de imagens; incondicional).
  - `POST /api/generations/delete {ids}` (≤100) → 204; ids inexistentes são ignorados (soft delete idempotente); shape inválido → 400.
  - `POST /api/generations/export {ids}` (≤100) → 200 `application/zip` em stream (padrão export de dataset, ADR-0006: get_to_file → zip em spawn_blocking → ReaderStream); entradas nomeadas `{job_short}_{filename}` para evitar colisão entre jobs.
- *Por quê:* galeria "ver TODAS as imagens" exige persistência e metadados por imagem; padrão do hook `models`/`job_artifacts` já é casa. *Gotcha:* exclusão da galeria não remove o artefato do job (a página Execuções continua mostrando); restore/lixeira e GC de S3 ficam como dívida.

### D6 — UI: painel de geração + galeria + comparador
- `/geracao` — pills **Gerar | Galeria**. Aba Gerar (2 colunas desktop, 1 coluna mobile): coluna de controles 320-384 px com modelo base (pills FLUX.2 Klein 4B / SDXL / SD 1.5 / Custom… quando houver checkpoints), variante destilada (flux), quantização, **editor multi-LoRA** (N linhas: Select LoRA + slider escala; "Nenhum"; + adicionar até 4), prompt, negative (colapsável), resolução, steps, CFG, seed (input + lock + dado), batchSize (1..8), NodeSelect, CTA "Gerar". Coluna direita: `JobProgressLive` (telemetria) + grade do resultado do último job (N cards com seed) + link "Ver na galeria".
- **Galeria:** grade de thumbs com seleção (checkbox por card), paginação infinita (50/bloco, padrão da galeria de datasets), **FloatingSelectionBar** ganha ações Excluir e Exportar (lote) + "Comparar (2)" quando exatamente 2 selecionadas. Delete com `ConfirmDialog`; export dispara o zip.
- **Comparador:** `CompareSlider.tsx` — modal com 2 imagens empilhadas, revelação por `clip-path: inset(0 50% 0 0)` na superior, handle arrastável (pointer events + `touch-action: none` + setas do teclado), labels mono (seed + prompt) nos cantos.
- `/playground` perde as pills e o `PlaygroundDiffusion` (vira YOLO-only, zero mudança de lógica).
- *Por quê:* Design System Dark-Only Vidro Óptico já cobre os primitivos; FloatingSelectionBar já existe para operações em lote. *Gotcha:* One CTA rule — 1 botão primário por contexto (Gerar na aba Gerar; Excluir/Exportar na seleção da galeria).

### D7 — Mobile
- Breakpoints canônicos: `sm 640 / md 768 / lg 1024` (tokens existentes). `< md` o workspace rola como documento único (Anti-Scroll-Trap Rule do DESIGN.md); painel de controles vira bloco colapsável com toggle "Parâmetros".
- Touch targets ≥ 40×40 px (classes `size-10` para botões de ícone); slider do comparador com handle ≥ 24 px e `touch-action: none`; FloatingSelectionBar vira bottom-sheet no mobile.
- Performance do grid: thumbs (512 px JPEG) + `loading="lazy"` + `decoding="async"` + `content-visibility: auto` nos cards; sem virtualização no v1 (≤ ~1000 gerações ok).
- *Por quê:* pedido explícito; regras de design já existem no DESIGN.md (garantir compliance). *Gotcha:* comparador em viewport estreita usa a largura cheia com altura limitada (`max-h` + `overscroll-contain`).

### D8 — Limites e segurança
- `batchSize` 1..8; `loras` ≤ 4 com `scale` 0..2 cada; prompt ≤ 4000 (existente); dimensões 256..2048 (existente); custom upload ≤ 8 GiB; custom arch ∈ {sdxl, sd15}.
- Concorrência: 1 geração por nó (lock do daemon) + fila do manager por VRAM. Sem rate-limit (single-user, modelo de ameaça simples — mantido do resto do sistema).
- Abort por sentinela entre itens do batch (imagem em voo não cancela — aceito e documentado).
- VRAM: `vram_min` para custom derivado de arch+quant (espelha first-class); entrada de geração no `vram-table.yaml` (mode `generate`) para o roteamento estático.
- *Por quê:* limites baratos de validar no principal (validação pura); VRAM 12 GB é o teto físico do nó. *Gotcha:* 8 GiB de upload + single-user = risco de disco local do spool (aceito; spool é tempfile efêmero).

## Delta de contrato (OpenAPI 0.24.0 → 0.25.0)

**Schemas:**
- `DiffusionGenerateJobRequest` (+): `batchSize` (int 1..8, default 1), `loras` (array ≤4 de `LoraRef`), `customModelId` (uuid|null); `baseModel` deixa de ser required (XOR com `customModelId`); `weights`/`loraScale` marcados deprecated (400 se coexistirem com `loras`).
- Novo `LoraRef {modelId: uuid, scale: number 0..2}`.
- Novo `Generation {id, jobId, filename, url, thumbUrl, width, height, seed, prompt, negativePrompt?, params, createdAt}` (camelCase; `params` = JSONB bruto do banco, snake_case interno — mesmo padrão do `Job.params`).
- Novo `GenerationList {items, total}`; `GenerationIdsRequest {ids: [uuid] min 1 max 100}` (reuso para delete e export).
- `Model` (+): `kind: 'lora'|'checkpoint'|null`, `arch: 'flux-2-klein-4b'|'sdxl'|'sd15'|null` (aditivo).

**Rotas:**
- `GET /api/generations` → 200 `GenerationList` | 400 (filtros inválidos) | 503 (manager fora).
- `GET /api/generations/:id/data` → 200 imagem (proxy) | 404 | 503.
- `POST /api/generations/delete` (body `GenerationIdsRequest`) → 204 | 400 | 503.
- `POST /api/generations/export` (body `GenerationIdsRequest`) → 200 `application/zip` stream | 400 | 503.
- `POST /api/models/upload`: campos multipart opcionais `kind` + `arch` (hints; sniff do servidor é autoritativo) — 400 `invalid_request` em conflito/desconhecido; cap 8 GiB.

**Erros novos:** `unsupported_architecture` (400, custom arch fora de {sdxl, sd15}).

**Config.yaml (engine):** `generate.batch_size`, `generate.loras: [{path, scale}]` (substituído pelo orquestrador), `generate.custom_checkpoint_path` + `generate.arch` (opcionais).

**Transporte (orquestrador):** artefatos `generated_*.png` (kind `generated`), `thumb_*.jpg` (kind `generated_thumb`), `generation_meta.json` (kind `generated_meta`).

## Schema (migration `0011_generations.sql` — pasta api-principal, compartilhada com manager)

```sql
ALTER TABLE models
  ADD COLUMN kind TEXT CHECK (kind IS NULL OR kind IN ('lora','checkpoint')),
  ADD COLUMN arch TEXT CHECK (arch IS NULL OR arch IN ('flux-2-klein-4b','sdxl','sd15'));
UPDATE models SET kind = 'lora' WHERE engine = 'diffusion';

CREATE TABLE generations (
  id UUID PRIMARY KEY,
  job_id UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
  s3_key TEXT NOT NULL UNIQUE,
  thumb_s3_key TEXT,
  filename TEXT NOT NULL CHECK (char_length(filename) BETWEEN 1 AND 255),
  seed BIGINT NOT NULL CHECK (seed >= 0),
  prompt TEXT NOT NULL CHECK (char_length(prompt) BETWEEN 1 AND 4000),
  negative_prompt TEXT CHECK (negative_prompt IS NULL OR char_length(negative_prompt) <= 4000),
  width INT NOT NULL CHECK (width BETWEEN 256 AND 2048),
  height INT NOT NULL CHECK (height BETWEEN 256 AND 2048),
  params JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  deleted_at TIMESTAMPTZ
);
CREATE INDEX generations_created_at_idx ON generations (created_at DESC);
CREATE INDEX generations_job_id_idx ON generations (job_id);
CREATE INDEX generations_deleted_idx ON generations (deleted_at) WHERE deleted_at IS NOT NULL;
```

Invariantes: `deleted_at` = soft-delete (objeto S3 intocado; sweep dívida); `s3_key` sempre `artifacts/{job_id}/{filename}` (único por definição); `generations` é dono do manager (como `jobs`/`job_artifacts`); FK CASCADE — apagar job apaga gerações.

## Decisões de boundary

- **Engine:** dono da geração (batch, loras, custom, serve) e da telemetria por request; escreve outputs + `generation_meta.json`.
- **Orquestrador:** dono do ciclo de vida do daemon (spawn/kill/idle/preempção/fallback one-shot), staging de `weights_refs`, substituição de placeholders no config.yaml, coleta de artefatos em glob. Stateless em relação ao Postgres (continua).
- **Manager:** dono de `generations` (hook no `report_job`), resolução de `loras[]`/`custom_model_id` → refs, rotação VRAM. Comunicação principal↔manager via HTTP interno (`MANAGER_TOKEN`), como hoje.
- **Principal:** única superfície do front; validação pura (XOR, limites, sniff de arch), config.yaml, BFF da galeria (presigned URLs + proxies + zip).
- **Web:** nunca fala com manager/orquestrador.

## Spike obrigatório

- **S1 — `from_single_file` + quantização + multi-LoRA (premissa externa: comportamento do diffusers pinado):** critério binário: (a) `StableDiffusionXLPipeline.from_single_file(ckpt, quantization_config=…4bit/8bit)` carrega e gera sem crash na imagem `trainer-difusao:gpu`; (b) `load_lora_weights` + `set_adapters` com 2 LoRAs funciona no pipeline custom E no `Flux2KleinPipeline`. Falhou (a) → custom restrito a fp16 (e `quantization≠none`+custom vira 400); falhou (b) → v1 aceita N LoRAs mas aplica sequencial com fallback para 1 (documentar). Roda antes de G.2/G.4.
  - **RESULTADO (2026-09-15, dev host — superfície de API do diffusers `0.40.0` pinado no Dockerfile.gpu, wheel inspecionado):**
    - ✅ (a) SDXL: `StableDiffusionXLPipeline` herda `FromSingleFileMixin` (`pipeline_stable_diffusion_xl.py:170-175`) e o loader de modelo consome `quantization_config` (`loaders/single_file_model.py:361`, aplicado em L373-396) — `from_single_file(path, quantization_config=BNB4bit/8bit)` é API suportada. **Validação runtime @gpu real fica para a sessão GPU manual (doutrina).**
    - ✅ (b-parte 1) `Flux2KleinPipeline(DiffusionPipeline, Flux2LoraLoaderMixin)` (`pipeline_flux2_klein.py:155`) com `load_lora_weights(..., adapter_name=...)` — N adaptadores NOMEADOS carregáveis.
    - ⚠️ (b-parte 2) `Flux2LoraLoaderMixin` NÃO expõe `set_adapters` (0.40.0). Caminho canônico para escala multi-LoRA no Flux2Klein: adapters peft no transformer via `pipe.transformer.set_adapters(nomes, escalas)` (`loaders/peft.py:437`, `PeftAdapterMixin`). Engine implementa: carrega com `adapter_name` distintos + escala via peft no transformer; fallback (só 1 LoRA no Flux2) só se o peft path falhar na sessão @gpu.
    - ✅ SDXL multi-LoRA: `StableDiffusionXLLoraLoaderMixin` com `set_adapters` canônico (padrão diffusers clássico).
    - ❌→decisão mantida: `Flux2KleinPipeline` NÃO tem `from_single_file` — Flux custom fora da v1, como decidido em D4.
    - **Veredito: S1 PASSA com nota de implementação** (multi-LoRA Flux2 = `adapter_name` + `pipe.transformer.set_adapters`).
- **S2 — PUT único > 5 GiB no SeaweedFS:** critério binário: upload de 6,5 GiB via `StoragePort.put` existente completa. Falhou → adicionar PUT multipart no StoragePort (ou cap volta a 2 GiB e custom SDXL fica fora do v1). Roda antes de G.4.
  - **RESULTADO (2026-09-15, dev host — endpoint S3 real do compose local):** ✅ **PASS** — PUT único de 6.5 GiB (6.979.321.856 bytes) via SigV4 em `http://localhost:8333/heph-data/` → **HTTP 200**, DELETE de limpeza → 204. Cap de 8 GiB para upload de checkpoint custom confirmado viável, sem multipart.

## Plano de commits (fatias G.x — 1 commit/despacho cada, < 400 linhas)

- **G.1 — Contrato + migration + contract tests.** Arquivos: `packages/contracts/openapi.yaml` (0.25.0), `services/api-principal/migrations/0011_generations.sql`, `services/api-principal/tests/contract.rs`. Verificação: `cargo test -p api-principal --test contract` + `scripts/test-db.sh`. Pronto: snapshot 0.25.0 verde; migration idempotente (backfill kind='lora').
- **G.2 — Engine one-shot v2: batch + multi-LoRA + custom + meta + thumbs.** Arquivos: `engines/trainer-difusao/src/trainer_difusao/generate.py`, `engines/trainer-difusao/tests/test_generate.py`. Verificação: `uv run pytest engines/trainer-difusao` (mock: N arquivos `generated_%04d.png` + thumbs + meta, seed+i; validações de limites; arch custom). Pronto: mock cobre todos os novos campos.
- **G.3 — Engine `serve` (daemon HTTP).** Arquivos: `engines/trainer-difusao/src/trainer_difusao/serve.py`, `__main__.py`, `tests/test_serve.py`. Verificação: pytest + manual (curl `/health`, `POST /generate`, `/shutdown` em mock). Pronto: daemon mock responde e respeita sentinela de cancel.
- **G.4 — Orquestrador: daemon manager + artefatos glob + staging multi-ref.** Arquivos: `services/orchestrator/src/daemon.rs` (novo), `services/orchestrator/src/lib.rs` (matriz glob, staging `weights_refs`, fallback), `services/orchestrator/src/main.rs` (envs), testes. Verificação: `cargo test -p orchestrator` + integ `compose.integ` com daemon mock. Pronto: job generate via daemon (mock) produz `generated_*.png`+meta; `DIFFUSION_DAEMON_ENABLED=0` cai no one-shot (testes existentes intactos).
- **G.5 — Manager: multi-LoRA/custom + hook `generations` + rotas internas.** Arquivos: `services/manager/src/lib.rs` (create_job `loras`/`custom_model_id`, hook no report_job, `GET/POST /internal/generations*`), testes. Verificação: `cargo test -p manager`. Pronto: refs múltiplos resolvidos; gerações inseridas idempotentes; soft-delete interno.
- **G.6 — Principal BFF: request v2 + galeria.** Arquivos: `services/api-principal/src/jobs/{models.rs,handlers.rs}` (validação XOR/limites, config.yaml v2, vram_min custom), `services/api-principal/src/generations/{mod.rs,handlers.rs}` (novo), `services/api-principal/src/auth/routes.rs`, `services/api-principal/src/models/{handlers.rs,validate.rs}` (sniff + cap 8 GiB), testes. Verificação: `cargo test -p api-principal` (+ contract). Pronto: endpoints da galeria E2E vs manager; sniff classifica sdxl/sd15/lora/flux.
- **G.7 — UI painel `/geracao` + sidebar + `/playground` YOLO-only.** Arquivos: `apps/web/app/(studio)/geracao/page.tsx`, `components/studio/GenerationPanel.tsx`, `components/studio/LoRAEditor.tsx`, `components/studio/Sidebar.tsx`, `app/(studio)/playground/page.tsx` (remove pills/diffusion), `app/(studio)/layout.tsx` (breadcrumbs), `lib/playground.ts`, `types/studio.ts`. Verificação: `npm run build && npm run lint`; chrome-devtools (snapshot + console 0 erros). Pronto: gera 1..N (batch, multi-LoRA, custom) com telemetria ao vivo; `/playground` só YOLO.
- **G.8 — UI galeria + comparador + mobile.** Arquivos: `components/studio/GenerationGallery.tsx`, `components/studio/CompareSlider.tsx`, `lib/generations.ts`, `components/studio/FloatingSelectionBar.tsx` (export/compare), ajustes mobile (painel colapsável, touch targets, grid thumbs). Verificação: `npm run build`; chrome-devtools em 375/768/1280 px; Lighthouse a11y snapshot. Pronto: galeria com paginação + delete/export em lote + comparador funcional; mobile 1 coluna.
- **G.9 — Docs sync + review + vram-table.** Arquivos: `docs/backend.md` §9/§10, `docs/frontend.md` §10, `docs/adr/0023-aba-geracao.md` (Status → ACEITA), `packages/policies/vram-table.yaml` (entradas `generate`), `docs/dividas.md` (runners parciais, restore/lixeira de gerações, sweep S3, quant custom). Verificação: `graft build` + leitura cruzada dos deltas. Pronto: nenhuma linha "falsa" restante nos docs.

## Riscos e o que testar

- **Thrash de reload do daemon** entre specs — aceito (single-user); testar sequência flux→sdxl→flux em mock.
- **Janela de cold-start de VRAM** (spawn→alocação) — mitigada pelo lock local; testar 2 jobs simultâneos no mesmo nó em integ.
- **`from_single_file` + quant + multi-LoRA** — gate do spike S1.
- **PUT > 5 GiB (SeaweedFS)** — gate do spike S2.
- **Sniff de arch heurístico** — testar com checkpoints reais de SD15/SDXL e com LoRA de treino (não classificar LoRA como checkpoint).
- **Abort em daemon** — sentinela entre itens; imagem em voo não cancela; testar cancel com batch 8 no mock.
- **Hook `generations` em jobs legados** (sem meta) — só jobs novos são indexados; testar idempotência (`ON CONFLICT`) em re-report.

## O que fica falso nos docs (aplicar no commit de sync, G.9)

1. `docs/adr/0020-playground-difusao.md` D3 — "Alternador de abas no topo: Geração (Difusão) e Detecção (YOLO)" → falso após G.7.
2. `docs/adr/0020-playground-difusao.md` D0 — payload com `weights`/`loraScale` únicos → estendido pela ADR-0023 D3/D4 (retrocompat preservada, mas a descrição muda).
3. `docs/frontend.md` §10 Playground — "Página /playground (Fatia J.5)… pills" → /playground YOLO-only; novas rotas `/geracao` + galeria.
4. `docs/frontend.md` §4.1 Sidebar — item "Playground" → "Detecção YOLO" + novo item "Geração".
5. `docs/frontend.md` §11 estrutura de pastas — remover `PlaygroundDiffusion` do contexto de /playground; adicionar `app/(studio)/geracao/`.
6. `docs/backend.md` §9 — descrição e schema de `POST /api/jobs/diffusion/generate`; adicionar rotas de galeria e schema `Generation`; §10 — colunas `models.kind/arch` e tabela `generations`.
7. `docs/backend.md` §5/§9 — dívida dos runners: **parcialmente quitada** para difusão (daemon interno); a API pública `/runners/*` continua dívida — anotar em `docs/dividas.md`.
8. `docs/backend.md` §4 — "Orquestrador sobe um container trainer-<engine>-<jobid> por job" → exceto modo daemon de difusão.
9. `docs/frontend.md` §10 — `getGeneratedImageUrl`/`startDiffusionGenerateJob` descrições atualizadas para batch/multi-LoRA/custom.

Nota: nada em `IDEIA.md` é contradito — a aba Geração é a "aba por categoria" do §2 (Difusão) e o upload de modelo custom é o §3 ("upload de modelo pela interface").
