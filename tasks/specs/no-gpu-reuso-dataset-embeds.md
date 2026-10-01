# Spec: Reaproveitar dataset e text-embeds entre jobs no nó GPU

**Fatia/branch:** `feat/no-gpu-reuso-dataset-embeds` · **Origem:** `tasks/backlog.md` (seção 3, item "Reaproveitar dataset e text-embeds") · **Nó:** `docker-04` (60 GB; `/data/outputs` com 39 GB em 12 jobs, medido 2026-10-01).

## 1. Contexto / Evidência

O pacote S3 já é reusado por fingerprint (`services/api-principal/src/jobs/prepare.rs:578`, `try_reuse_package`). O desperdício está no nó GPU.

| Local | Achado |
|---|---|
| `services/orchestrator/src/app/mod.rs:250-307` | `run_job_inner` baixa `dataset.zip` (3.7 GB) em `temp_dir` e extrai com `unzip_safe()` em `datasets/datasets-cache/<job_id>/` — por job, sem dedup. |
| `crates/heph-contracts/src/dispatch.rs:5-9` | `PackageRef { key, md5_zip, bytes }` já traz `md5_zip` → chave de dedup disponível sem mudar contrato. |
| `services/orchestrator/src/adapters/executor_docker.rs:41-43` | Mounts `host:container` sem `ro`: engines escrevem no dataset. |
| trainer-yolo (ultralytics) | Escreve `labels.cache` dentro do diretório do dataset → dataset compartilhado não pode ser montado read-only nem usado diretamente. |
| trainer-difusao / trainer-clip / autolabel | Só leem o dataset; escrevem em `outputs/`. |
| `engines/trainer-difusao/src/trainer_difusao/common_pkg/text_embeds.py:15-20` | `TextEmbedsCache` em `{output}/text_embeds_cache/`. |
| `engines/trainer-difusao/src/trainer_difusao/common_pkg/train_config.py:124-126` | Chave = só `sha256(caption)[:16]` (sem encoder/quantização/seq len/dtype). |
| `engines/trainer-difusao/src/trainer_difusao/models/loop.py:347`, `models/qwen_image.py:449` | Pontos de construção do `TextEmbedsCache`. |
| `services/manager/src/jobs/resolve.rs:70-103` | Resume resolve checkpoints pelo S3 (`job_artifacts` kind `checkpoint`/`model`, key `artifacts/{job_id}/{path}`); não depende do disco do nó. |
| `services/manager/src/reporting/artifacts.rs:42-75` | `save_intermediate_artifacts`: checkpoints por época são enviados e registrados DURANTE `running` → jobs cancelados TÊM checkpoints no S3 (ex.: `0962380c` retomado do artefato `5b82f426` após cancel). |
| `services/orchestrator/src/app/mod.rs:1200-1346`, `:1365-1413` | Upload final ao S3; falha → job falha; sucesso → report `done` e limpeza só de `temp_dir` (`outputs/<job>` persiste). |
| `services/orchestrator/src/adapters/sweeper.rs:224-235` | Remove `datasets-cache/<job_id>` com idade > 24h (único GC atual). |
| `infra/compose.gpu.yaml:97-104` | Volumes `gpu_datasets:/data/datasets`, `gpu_outputs:/data/outputs`. |

Medições: `0962380c` = 9.2 GB, dos quais 7.3 GB de `text_embeds_cache`; resume `a65f012e` +8.7 GB (7.3 GB de embeds idênticos recalculados).

## 2. Decisões (usuário, autoritativas)

- **Escopo:** pilares A+B+C juntos numa única fatia/branch, com um único smoke GPU.
- **Orçamento de datasets:** `DATASET_CACHE_MAX_GB=15` (env, default 15); eviction LRU que NUNCA remove entrada em uso.
- **Outputs:** sweeper purga o conteúdo pesado de `outputs/<job>/` com TTL de 24h após estado terminal (`OUTPUT_PURGE_TTL_SECS`, default 86400), apenas arquivos com upload ao S3 confirmado.
- **Legado docker-04** (39 GB em `/data/outputs` + `datasets-cache/<job>` antigos): limpeza única pelo `@infra` dentro da fatia, após conferir no S3/`job_artifacts` que cada arquivo existe; checkpoints de `0962380c` e `3ea78a5c` preservados até essa conferência.
- **Cutover limpo:** sem caminho legado paralelo para `datasets-cache` por job.

## 3. Design

### A. Cache de dataset por `md5_zip` (orchestrator)

- **Layout:** `datasets/datasets-dedup/<md5_zip>/` (entrada compartilhada) + visão por job `datasets/datasets-cache/<job_id>/`.
- **Single-flight:** o orchestrator é processo único por nó → mutex async em processo, chaveado por `md5_zip`, cobrindo download + validação MD5 + extração + promoção. Sem arquivos de lock/refcount em disco.
- **Miss:** baixa, valida `md5_zip`, extrai para `datasets-dedup/.tmp-<uuid>/`, promove com `rename` atômico para `datasets-dedup/<md5_zip>/`. `.tmp-*` remanescentes são varridos no boot.
- **Hit:** pula download/extração; log `dataset cache hit`; atualiza mtime da entrada (base do LRU).
- **Visão por job:** `cp -al` (hardlink de arquivos, diretórios novos) de `datasets-dedup/<md5_zip>/` para `datasets-cache/<job_id>/`; o container continua vendo o mesmo path de hoje.
- **Invariante:** engines podem CRIAR arquivos na visão por job (ex.: `labels.cache` do ultralytics), mas NUNCA modificar arquivo existente in-place — isso corromperia o inode compartilhado. Verificado por teste/smoke (md5 da entrada dedup inalterado após job YOLO).
- **Em uso:** conjunto dos `md5_zip` dos `active_jobs` do orchestrator.
- **Eviction:** sweeper remove entradas por LRU (mtime) enquanto o total exceder `DATASET_CACHE_MAX_GB`, pulando qualquer `md5_zip` em uso. Visões por job continuam sendo limpas pelo sweeper existente.
- Todos os diretórios criados via `create_dir_all_open` (PITFALLS:51).

### B. Text-embeds compartilhado (engines + orchestrator)

- **Localização (mecanismo único):** orchestrator passa env `TEXT_EMBEDS_CACHE_DIR=/outputs/.text_embeds_cache/` nos docker args do container de engine (volume de outputs). Sem a env, a engine usa `{output}/text_embeds_cache` (fallback atual).
- **Chave:** diretório de namespace derivado de tudo que muda o embedding — id do modelo base/text-encoder (+ md5 do text encoder custom quando `text_encoder_ref` presente), quantização, max sequence length/configuração do tokenizer, dtype. Nome do arquivo segue `sha256(caption)`.
- **Escrita atômica:** tmp + `rename` (dois jobs podem gravar a mesma chave).
- **Pontos de mudança:** `common_pkg/text_embeds.py:15-20`, `common_pkg/train_config.py:124-126`, `models/loop.py:347`, `models/qwen_image.py:449`.
- **Eviction:** sweeper do orchestrator aplica LRU por orçamento `TEXT_EMBEDS_CACHE_MAX_GB` (default proposto 10, ajustável).

### C. Purga de `outputs/<job>/` (orchestrator)

- Sem migração ou endpoint novo no manager; sem mecanismo emergencial por pressão de disco.
- **Manifesto de upload:** após cada upload bem-sucedido (intermediário ou final), o orchestrator registra o path relativo em um manifesto local do job.
- **Sweeper:** para job em estado terminal há mais de `OUTPUT_PURGE_TTL_SECS`, remove de `outputs/<job>/` apenas: arquivos listados no manifesto, `text_embeds_cache/` e staging `weights/`.
- **Mantém sempre:** `config.yaml`, `metrics.jsonl`, `logs/telemetry.jsonl` e qualquer arquivo nunca enviado ao S3.
- Resume continua funcionando após a purga, pois resolve checkpoints pelo S3 (`resolve.rs:70-103`).

## 4. Contratos

Sem mudança de API pública nem de `heph-contracts`.

| Env | Dono | Default | Uso |
|---|---|---|---|
| `DATASET_CACHE_MAX_GB` | orchestrator | 15 | Orçamento LRU de `datasets-dedup/` |
| `OUTPUT_PURGE_TTL_SECS` | orchestrator | 86400 | TTL pós-terminal para purga de `outputs/<job>/` |
| `TEXT_EMBEDS_CACHE_MAX_GB` | orchestrator | 10 (ajustável) | Orçamento LRU de `.text_embeds_cache/` |
| `TEXT_EMBEDS_CACHE_DIR` | orchestrator → engine | `/outputs/.text_embeds_cache/` (engine: fallback `{output}/text_embeds_cache`) | Diretório compartilhado de embeds |

Paths: `datasets/datasets-dedup/<md5_zip>/`, `datasets/datasets-dedup/.tmp-<uuid>/`, `datasets/datasets-cache/<job_id>/`, `outputs/.text_embeds_cache/<namespace>/<sha256(caption)>`, manifesto de upload em `outputs/<job>/`.

## 5. Ondas (aceite binário)

| Onda | Dono | Entrega | Aceite |
|---|---|---|---|
| W1 | `@backend` (orchestrator) | Pilar A; env/mount de `TEXT_EMBEDS_CACHE_DIR`; sweeper de C + manifesto; eviction de embeds | Testes passam: hit/miss; single-flight concorrente (uma extração para N jobs); eviction pula `md5_zip` em uso; purga pula arquivos fora do manifesto e mantém keepers; `.tmp-*` varrido no boot. |
| W1 (paralelo) | `@engines` (trainer-difusao) | Pilar B: leitura da env, chave com namespace, escrita atômica | Testes passam: namespace muda com encoder/md5 custom, quantização, seq len, dtype; fallback sem env. |
| W2 | `@infra` | Vars em compose/`env.gpu`; rebuild `orchestrator-gpu` + `trainer-difusao:gpu` no docker-04; limpeza legada conferida contra S3/`job_artifacts` | `docker compose config -q` ok; serviços rodando com as envs; legado removido só após conferência, `0962380c`/`3ea78a5c` preservados até ela. |
| W3 | Coordenador + `@reviewer` + `@docs` | Smoke (§6), gate do reviewer, atualização de PITFALLS/REPO_MAP/docs de storage | Todos os itens do smoke passam; veredito do `@reviewer`; docs atualizadas. |

## 6. Smoke (docker-04, binário)

Com TTLs pequenos via env:

1. Dois jobs de difusão consecutivos no mesmo dataset → job 2 sem fase `downloading_dataset` e com log `dataset cache hit`; `du -sh datasets-dedup/` mostra uma única entrada.
2. Job 2 registra hits de text-embeds; `du` de `.text_embeds_cache/` não cresce ~7.3 GB.
3. Um job YOLO sobre dataset em cache → md5 da entrada dedup idêntico antes/depois.
4. Após o TTL, sweeper purga arquivos pesados já enviados e mantém keepers.
5. Resume a partir de job purgado funciona (checkpoint via S3).
6. Logs/métricas crus inspecionados no nó pelo coordenador/reviewer (PITFALLS:85).

## 7. Riscos

- Engine que reescreva arquivo existente in-place corrompe a entrada dedup (mitigação: invariante + verificação de md5 no smoke).
- Namespace incompleto de embeds gera reuso de embedding errado silenciosamente (mitigação: testes por dimensão da chave).
- Manifesto ausente/incompleto apenas preserva arquivos (falha segura); manifesto com path não enviado causaria perda local — só gravar após upload confirmado.
- Limpeza legada é irreversível: exige conferência arquivo a arquivo contra S3/`job_artifacts`.
- Hardlinks exigem `datasets-dedup/` e `datasets-cache/` no mesmo filesystem (mesmo volume `gpu_datasets`).
