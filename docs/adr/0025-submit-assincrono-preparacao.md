# ADR-0025 — Submit assíncrono com preparação em background

Data: 2026-09-17 · Status: ACEITA (usuário; bug "empacotamento síncrono mata
submits em datasets grandes") · Fatia: `feat/jobs-async-submit` (P1 = contrato+ADR)

## Contexto

`POST /api/jobs/{yolo,autotracker,autolabel,diffusion,predict}` em
`services/api-principal/src/jobs/handlers.rs` executam `build_package*` DENTRO
do request: baixa todas as imagens do S3 serialmente, zipa, lê o zip inteiro em
RAM p/ md5 e faz PUT — num dataset de 860 imagens/3GB estoura o timeout de 30s
do proxy Next ⇒ 500 no browser e o job nunca é criado no manager. O aceite tem
que custar <1s; o empacotamento vira trabalho de fundo com progresso visível.

## Decisões

- **D0 — Aceite em duas fases.** O request faz só validação barata (<1s);
  o manager aloca o `jobId` e cria o registro com `status='preparing'`,
  `package_ref=null` + `params.prepare{...}`. O BFF responde
  `202 {jobId, status:preparing, queuePosition:null}` e agenda a preparação num
  `tokio::spawn` NO api-principal — único dono de `StoragePort`+pool por
  isolamento; manager/orchestrator ficam de fora.
- **D1 — Worker de prep com reuso por fingerprint.**
  `sha1(dataset_id | image_ids ordenados ou 'ALL' | count ativas |
  max(images.updated_at) | max(boxes.updated_at) | classes | trigger_word |
  engine)` → tenta reuso de `dataset_versions` existente via
  `manifest->>'fingerprint'`; senão build otimizado → conclui com
  `POST /internal/jobs/:id/prepare-complete {datasetVersionId, packageRef}`
  (`preparing→queued`) ou `prepare-fail {code,message}`
  (`preparing→failed`, `error='prepare_failed:<motivo>'`).
- **D2 — Progresso pelo canal ADR-0024.** Durante o prep o worker reporta
  `{status:"preparing", phase:"packaging_dataset", message, progress}`; fases
  `packaging_*` nascem no BFF, `staging_*` no nó (AC-007 futuro).
- **D3 — Persistência e robustez.** Nova tabela `job_prepares` no principal
  (migration `0015`, detalhe na P2). Dedupe: submit com mesmo fingerprint e job
  em `preparing` (janela 30min) retorna o mesmo `jobId`. Abort em `preparing` →
  `cancelling` (worker observa a flag); DELETE em `preparing` → 409 (inalterado).
  Watchdog no manager: `preparing` > 60min sem update → `failed`
  (`prepare_timeout`); recovery no boot do principal re-spawna prepares
  (`attempts<3`).
- **D4 — Compensation e rollback.** NUNCA apagar pacote referenciado por job
  aceito; GC ≥7 dias no manager; sweep manual de prefixos órfãos >48h. Rollback:
  manager aceita `package_ref` presente ⇒ caminho legado `queued` continua;
  ordem de deploy manager→principal.

## Consequências

- Contrato público minor (`openapi 0.29.0`): `SubmitJobResponse.status` ganha
  `preparing`; `Job.error` pode ser `prepare_failed:<motivo>`; abort cobre
  `preparing`. Rotas `/internal/*` NÃO entram no contrato público.
- Front passa a tratar `preparing` como "pacote em construção" (progresso via
  `phase=packaging_*`); nenhum polling novo — SSE/eventos já existentes.
- P2 implementa: migration 0015, `params.prepare`, endpoints internos,
  watchdog, recovery e build otimizado (streaming+concorrência no S3).
