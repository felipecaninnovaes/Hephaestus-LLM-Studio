# ADR-0016 — AutoLabel v1 (mock, local): geração de legendas em lote + retorno de captions ao Postgres (Fatia AutoLabel)

- **Status:** ACEITA (2026-09-11 — aceite explícito P1–P4). Especificação executável da fatia.
- **Data:** 2026-09-11
- **Componentes:**
  - `services/api-principal`: rota de submit `POST /api/jobs/autolabel` (202) + rota de apply `POST /api/jobs/:id/autolabel/apply` (200), migration `0009_captions_autolabel.sql` (`captions.origin` com `'autolabel'`), validação de request, leitura S3 de `captions.jsonl` e ingest com merge dirigido por origem.
  - `services/manager`: aceita `engine='autolabel'`, `mode='autolabel'` e repassa `orchestrator_hint` (reuso genérico da esteira de jobs e da Fatia N).
  - `services/orchestrator`: matriz `(engine, mode)` ganha `("autolabel", "autolabel")`, coleta do artefato `captions.jsonl` (kind: `captions`).
  - `engines/trainer-yolo`: subcomando `autolabel` com mock determinístico gerando `captions.jsonl` sob `ENGINE_MOCK=1` (reuso da imagem do runner local, padrão ADR-0008 D2).
  - `apps/web`: botão AutoLabel habilitado na galeria (`/datasets/[id]`) para datasets com imagens (`imagesCount > 0`), modal `AutoLabelModal.tsx` com `NodeSelect`, botão "Aplicar Legendas" no card de detalhe em `/jobs`.
  - `packages/contracts`: spec OpenAPI bumped de 0.14.0 para 0.15.0.
- **Fontes:**
  - `IDEIA.md` §1/:9 (abrir galeria para acessar AutoLabel e AutoTracker), §2/:32-33 (Difusão e OpenCLIP com preparo de dados via AutoLabel), §3/:41-47 (AutoLabel gera descrições de imagens; modelo local ou formato OpenAI API).
  - `docs/backend.md` §4/:56 (`autolabel` no enum de jobs), §10/:240-245 (schema `captions`).
  - `docs/frontend.md` §4.3/:74, §7.2/:175-180 (`autolabel-workspace`, prompt caption, modelo local vs API).
  - `docs/adr/0008-autotracker-v1.md` (R7: dívida `captions.origin` sem `'autolabel'`; padrão D0 job assíncrono + D1 apply explícito).
  - `docs/adr/0015-visibilidade-selecao-no.md` (D1 seletor de nó `orchestratorId` e fallback automático).

---

## 1. Contexto e Problema

O AutoTracker preparou os dados para modelos de detecção (YOLO). Para fechar o ciclo de Difusão (LoRA/Flux/SDXL) e OpenCLIP, o sistema precisa de anotações textuais (legendas/captions).

A tabela `captions` já existe no Postgres desde a migration 0003, assim como os endpoints manuais (`PUT /api/datasets/:id/images/:image_id/caption`), a exportação `captions.jsonl` (Fatia 3e) e os gatilhos de contagem (`heph_refresh_dataset_counters`). O que falta é a geração automatizada em lote:

1. Como o job é disparado e orquestrado (D0).
2. Como as legendas retornam ao Postgres com segurança e consentimento (D1).
3. O ajuste no schema para admitir a origem `'autolabel'` (D2).
4. O engine executor com modo mock determinístico (D3).
5. A UX na galeria e no histórico de execuções (D4).

---

## 2. Decisões Numeradas

### D0 — Execução como Job Assíncrono (POST /api/jobs/autolabel)

- O AutoLabel v1 corre como job na fila central do Manager: `POST /api/jobs/autolabel` → 202 Accepted.
- Reusa o empacotamento de imagens (`build_package`), montando o zip do dataset com suas imagens.
- Aceita `orchestratorId` opcional, repassado como `orchestrator_hint` ao Manager (herança direta da Fatia N).
- Body do request:
  ```json
  {
    "datasetId": "uuid",
    "prompt": "instrução opcional de caption",
    "model": "mock",
    "orchestratorId": "uuid opcional"
  }
  ```
- Gotcha: Requer dataset com ao menos 1 imagem ativa (`images_count > 0`), retornando 409 `dataset_not_ready` se vazio.

### D1 — Retorno via artefato captions.jsonl e Apply Explícito (POST /api/jobs/:id/autolabel/apply)

- O engine emite um artefato `captions.jsonl` em `artifacts/<job_id>/captions.jsonl`:
  ```json
  {"filename": "img_0001.jpg", "caption": "Uma foto em close de uma placa de circuito impresso com solda fria"}
  {"filename": "img_0002.jpg", "caption": "Placa de circuito verde com conectores banhados a ouro"}
  ```
- O Orquestrador faz o upload do artefato no S3 no report `done`.
- O cliente chama `POST /api/jobs/:id/autolabel/apply` com `{ overwrite?: boolean }`:
  1. O api-principal valida o job (status `done`, engine `autolabel`).
  2. Baixa o `captions.jsonl` do bucket S3 via `StoragePort`.
  3. Mapeia `filename` para as `images` ativas do dataset correspondente.
  4. Executa UPSERT na tabela `captions`:
     - Se `overwrite=false` (default): só atualiza imagens que não têm caption ou cuja caption atual tem `origin='autolabel'` (preserva anotações manuais e importadas).
     - Se `overwrite=true`: sobrescreve inclusive anotações de origem `manual` e `import`.
  5. Retorna `{ applied: number, skipped: number, images: number }`.

### D2 — Migration 0009: captions.origin CHECK constraint

- Ajuste da constraint em `services/api-principal/migrations/0009_captions_autolabel.sql`:
  ```sql
  ALTER TABLE captions DROP CONSTRAINT IF EXISTS captions_origin_check;
  ALTER TABLE captions ADD CONSTRAINT captions_origin_check
      CHECK (origin IN ('manual', 'autolabel', 'autotracker', 'import'));
  ```

### D3 — Engine e Matriz do Orquestrador

- Matriz de despacho `(engine, mode)` em `services/orchestrator/src/lib.rs` ganha `("autolabel", "autolabel")`.
- Coleta de artefatos: `[("captions.jsonl", "captions"), ("metrics.jsonl", "metrics")]` (onde `metrics.jsonl` é opcional/tolerado ausente).
- O engine sob `ENGINE_MOCK=1` lê as imagens do pacote e gera legendas determinísticas baseadas no prompt enviado (ou descrição mock rica caso prompt vazio). Subcomando `autolabel` hospedado em `engines/trainer-yolo` para reuso direto da imagem `hephaestus/trainer-yolo:local` sem desvio de infra (padrão ADR-0008 D2).

### D4 — UX: Galeria e Execuções

- Galeria (`/datasets/[id]`): O botão "AutoLabel" é habilitado para datasets que possuam imagens (`imagesCount > 0`).
- Modal (`AutoLabelModal.tsx`):
  - Prompt da Legenda: instrução/estilo opcional (ex: "Fotografia realista de...").
  - Modelo: fixo "mock" na v1 (com placeholder para VLM local / OpenAI API).
  - Nó de Execução: componente `NodeSelect` com fallback visual.
  - Submissão → 202 Accepted → redireciona para `/jobs?job=<id>`.
- Página `/jobs`: Quando o job atinge `status === 'done'`, exibe o botão primário "Aplicar Legendas" que chama o endpoint de apply e emite toast de sucesso.

### D5 — Contratos e Erros

- Bump de especificação OpenAPI: 0.14.0 → 0.15.0.
- Erros padronizados:
  - 400 `invalid_request`: datasetId inválido ou orchestratorId não-UUID.
  - 404 `not_found`: dataset ou job inexistente.
  - 409 `dataset_not_ready`: dataset sem imagens ativas.
  - 409 `job_not_done`: tentativa de apply em job não finalizado.
  - 503 `queue_unavailable`: falha de comunicação com o Manager.

---

## 3. Plano de Commits da Fatia (feat/autolabel-v1)

| Passo | Escopo | Arquivos-Alvo | Critério de Aceite |
|---|---|---|---|
| **AL.0** | `docs(adr)` | `docs/adr/0016-autolabel-v1.md`, `docs/coordenacao.md` | ADR registrada como ACEITA e plano ativo gravado. |
| **AL.1** | `feat(api)` | `migrations/0009_captions_autolabel.sql`, `datasets/models.rs`, `datasets/handlers.rs` | Migration aplicada; captions aceita origin='autolabel' no DB e testes de integração passam. |
| **AL.2** | `feat(engine)` | `engines/trainer-yolo/src/trainer_yolo/autolabel.py`, `tests/test_autolabel.py` | Subcomando autolabel gera captions.jsonl determinístico sob ENGINE_MOCK=1. Pytest verde. |
| **AL.3** | `feat(orchestrator,manager)` | `services/orchestrator/src/lib.rs`, `services/manager/src/lib.rs` | Matriz ("autolabel", "autolabel"), coleta captions.jsonl e repasse orchestrator_hint. Testes unit/db verdes. |
| **AL.4** | `feat(api)` | `services/api-principal/src/jobs/` (handlers, models, routes), `packages/contracts/openapi.yaml` | Rota submit POST /api/jobs/autolabel (202) e apply POST /api/jobs/:id/autolabel/apply (200). Spec 0.15.0. Testes unitários e de contrato verdes. |
| **AL.5** | `feat(web)` | `apps/web/` (`AutoLabelModal.tsx`, datasets galeria, jobs detalhe, lib/autolabel.ts, types) | Botão AutoLabel ativo na galeria para imagesCount > 0; modal com NodeSelect; botão "Aplicar Legendas" no card de jobs done. Build web verde. |
| **AL.6** | `test(e2e)` | Testes integrados / smoke | Pipeline ponta-a-ponta testada e verificada: submit 202 -> done -> captions.jsonl -> apply -> captions no DB. |
| **AL.7** | `review` | Monorepo diff | Auditoria completa contra ADR-0016. |
| **AL.8** | `docs(sync)` | `docs/backend.md`, `docs/frontend.md`, `docs/coordenacao.md`, `docs/dividas.md` | Documentação sincronizada e dívidas atualizadas. |
