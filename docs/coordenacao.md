# Coordenação — estado do plano (memória do coordenador)

Arquivo de trabalho do agente coordenador: registra **onde estamos** e **qual o
próximo passo na ordem**, para sobreviver a restart de sessão. Não duplica
docs — referencia por seção. Atualizar: ao abrir fatia, ao fechar fatia, e ao
ser interrompido no meio de uma.

## Protocolo de retomada (início de sessão)

1. Ler este arquivo → seção "Plano em andamento".
2. `git status` + `git log --oneline -5` para conferir se o disco bate com o
   registrado (branch aberta, commits pendentes de push).
3. `graft check` se for mexer em código indexado (refresh: `graft build`).
4. Fontes de verdade para a fatia: `IDEIA.md`, `docs/backend.md` §9/§10,
   `docs/frontend.md` §10, `docs/repo-estrutura.md` (ordem de fatias),
   `docs/dividas.md` (dívidas a honrar no nascedouro) e os ADRs em
   `docs/adr/` — a 3b tem especificação própria e completa em
   **`docs/adr/0003-object-storage-s3.md`** (decisões D0–D10, delta de contrato,
   contorno da migration 0003, plano de commits 3b.0–3b.8); não reinvente nada que já
   está lá, e não aplique os deltas de `backend.md`/`frontend.md` antes do commit 3b.8.

## Estado atual — 2026-09-07 (sessão 10: FATIA 3e — ADR-0006 ACEITA pelo usuário com substituição consentida `replace`; 3e.1 Export despachado ao @rust-dev)

- **Sessão 7 foi CANCELADA pelo usuário no meio e reiniciada** — causa-raiz: o coordenador misturava decisão com execução (fixes de CI/fmt/compose/web feitos por ele), o contexto saturou e a sequência de fatias divergiu (3g pulou na frente; depois começou a planejar 3e fora de hora). O registro desatualizado foi o sintoma visível.
- **Merges feitos pelo usuário**: `feat/semantic-search` (3f inteira, `43ab513`) e `chore/ci-ownership` (`f37e524` — CI tem dono = `@infra-dev`; reviewer checa coerência ci/compose). Main sincronizada com origin; branches de fatia apagadas. **Sequência informada pelo usuário: 3e → 3f → 3h** — 3f FEITA (fora de ordem), **3e continua PENDENTE**; definição da 3h a confirmar com o usuário/roadmap.
- **REGRAS NOVAS DE PROCESSO (sessão 8, pedidas pelo usuário — valem para sempre)**:
  1. **Nenhum fix pelo coordenador — regra absoluta**: toda correção (build/teste/lint/UI/docs/config/charters), por menor que seja, mesmo fora de fatia, é ESPECIFICADA pelo coordenador e EXECUTADA pelo `@fixer`. Sem exceção. Charters atualizados: `hephaestus.md` (bullet de roteamento reescrito + passos 5/6 do fluxo), `fixer.md` (escopo estendido a edição mecânica em sessão de manutenção), `rust-dev.md` (`cargo fmt --all` obrigatório antes de reportar — lição 3g).
  2. **Registro por passo executado**: atualizar este arquivo a cada passo fechado, não só ao abrir/fechar fatia — registro desatualizado é o primeiro sintoma de sobrecarga.
  3. **CI tem dono**: run falho é problema de infra — despachar diagnóstico/correção ao `@infra-dev`, não ao coordenador.
  4. **Adoções do doc de governança estudado pelo usuário** ("Especificação de Requisitos e Diretrizes Arquiteturais", ~/DEV — aprovadas 2026-09-07, aplicadas nos charters): **regra das duas correções** (2 falhas do mesmo problema = contexto contaminado → registrar e escalar: design → `@architect`, sessão → propor "documentar e limpar"); **file ownership** (despachos paralelos só com arquivos disjuntos; contratos/openapi/migrations sequenciais antes); **sintomas de saturação** (registro atrasado, fixes acumulando, assuntos misturados = propor reset, não empurrar). O resto do doc JÁ praticamos (permissões, commits ≤400, mise+uv, hub-and-spoke, graft ≈ funil L0→L3) e alguns pontos foram REJEITADOS conscientemente (tipos `style/perf/build/ci` no commitlint, branch `edit/`, rebase preferencial, REPO_MAP.md/Makefile duplicando graft/scripts, guardian em background — lefthook + reviewer cobrem).
- **Dívida de logging REFINADA** (`docs/dividas.md`): desenho técnico apensado — `tracing`/JSON no Rust, `x-request-id`/`request_id` propagado principal→manager→orchestrator→engines, `/ready` readiness (db/storage/embedder) além do `/health` existente. Nada implementado — desenho-alvo da fatia de logging.
- **Branch `chore/agent-no-fix-rule`**: 3 charters editados pelo `@fixer` (primeiro despacho sob a regra nova — 5 substituições; verificação do coordenador OK: só os 3 arquivos tocados; único "você mesmo" restante no charter = RODAR checks, que é verificação, não execução). Push/merge = usuário.
- **Propostas AINDA pendentes de aprovação do usuário** (da auditoria de sobrecarga): script `scripts/ci-watch.sh` (matriz de runs/jobs via API Gitea — mata o polling manual) e smokes E2E versionados `scripts/smoke-*.sh` (o smoke das sessões 5–7 foi reescrito à mão a cada sessão).
- Ambiente herdado da sessão 7: compose de pé (db pg16-trixie, seaweedfs, manager, principal com código 3f, embedder healthy), dev server :3000, Chrome :9222.

### Sessão 10 — fatia 3e ABERTA (2026-09-07 — perguntas fechadas; 3e.0 ADR-0006 em curso)

- Protocolo de retomada conferido: main sincronizada com origin; disco bate com o registrado.
- Plano da 3e **commitado no tronco** (`f2f3a99`, `docs(coord)` — doc de coordenação, exceção autorizada).
- **Usuário fechou as 6 perguntas da §7 do plano: "tudo como você recomenda"** →
  (1) `POST /:id/package` (manifest p/ orquestrador) vai para a **fatia 4** (revisão consciente do D9 da ADR-0003);
  (2) Importar na UI **só na galeria** (nada no header de `/datasets`);
  (3) conflito de nome no import = **409 `slug_conflict` seco**, sem auto-sufixo;
  (4) limite do zip de import = **200 MiB + envelope 8 MiB** (padrão do upload 3b);
  (5) export inclui **somente imagens ativas** (`deleted_at IS NULL` — lixeira fora);
  (6) **sem migration** na 3e (schema já tem `origin='import'` desde a 3b) — ADR-0006 fecha explicitamente.
- **3e.0 FECHADO**: ADR-0006 escrita pelo `@architect` (`docs/adr/0006-export-import.md`, formato da casa: D0–D9, delta OpenAPI, testes, riscos R1–R10, plano de commits) e AUDITADA por mim linha a linha. Primeira versão → usuário impugnou o P3 original (409-seco); **emenda: substituição consentida** — 409 `slug_conflict` = protocolo de detecção (servidor NUNCA substitui sozinho) → diálogo de irreversibilidade na UI → re-envio com `replace=true` → teardown+ingest com dataset_id novo. Teardown = DELETE antigo + INSERT novo + classes na MESMA transação (rollback devolve o antigo intacto; UNIQUE liberado intra-tx); sweep do prefixo antigo pós-commit; validação COMPLETA do pacote antes de qualquer teardown (zip corrompido nunca destrói o existente — testado); R10 = janela destrutiva pós-commit (estado de falha: dataset ausente + zip intacto + resíduo reapável); embeddings antigas morrem no CASCADE (0004, verificado no código). **ACEITA pelo usuário (2026-09-07)**. Decisões extras que ficaram de pé (não impugnadas): campo `title` opcional no multipart; dedupe intra-zip → 400 `import_invalid`; `manifest.json` = fonte da verdade do roundtrip (labels/`dataset.yaml`/`captions.jsonl` são derivados que o import IGNORA — R8); `origin` das boxes preservado literal (não forçado a `import` — `autoTracked` não falseia). Sem spike (crates estáveis; critérios de inversão embutidos no 3e.1, fallback `async_zip`).
- **3e.1 FECHADO** (`21b95b9` na `feat/datasets-export-import`): Export completo pelo `@rust-dev` — `export.rs` novo (1052 linhas, 3 fases da D2; 11 tests), `StoragePort.get_to_file` (port+mock+s3), rota em `PROTECTED_ROUTES` `[200,401,404,503]`, openapi **0.6.0** declarando SÓ export (import fica para o 3e.2 — contract≡router a cada commit), Cargo: `zip` 2/`tokio-util`/`serde_yaml` promovido. Verificação do coordenador: fmt/check verdes, 76 lib + 11 contract + 7 search, spot-check do diff (3 fases, `deleted_at IS NULL` nas 3 queries, Stored/Deflated, Content-Disposition), escopo limpo (handlers.rs intocado, sem migration/manager/orchestrator). Smoke parcial "zip baixa" adiado para o 3e.verificar (roundtrip completo no fim da fatia).
- **3e.2 FECHADO** (`f37d76f`): Import completo pelo `@rust-dev` — `import.rs` novo (903 linhas: pré-scan zip-slip/bomb com reader contado, validação completa do manifest ANTES de qualquer write, teardown condicional `replace=true` fundido DELETE antigo+INSERT novo+classes na MESMA tx, sweep do prefixo antigo pós-commit, ingest objeto→linha→compensação, 409 pós-validação, 201 + indexação fire-and-forget), `models.rs` +260 (validações puras + 4 tests), `IMPORT_BODY_LIMIT_BYTES` (200 MiB + 8 MiB) no padrão 3b, `import_invalid` no error.rs, MockStorage ganhou `fail_after_puts(n)` (injeção de falha), openapi 0.6.0 com rota import + enum. Testes-db: 6 novos (`t3e_import_*`) — roundtrip fidelidade, substituição, 409 sem replace, zip-inválido+replace não destrói, manifest corrompido, falha PUT no meio. **Achado da verificação do coordenador**: o roundtrip falhava ~2/5 runs — `boxes[0]` assumia ordem de inserção, mas o import insere boxes num único `unnest` (created_at constante) e o detail ordena `ORDER BY id` (UUID) → ordem no wire não-determinística (a dívida "ordem de boxes" da 3d expondo o teste). **Fix (`@fixer`)**: teste compara por IDENTIDADE (trackId 7 = autotracker; null = manual) — 4× 54/54 verdes. A dívida segue em dividas.md (sem fatia). Verificação final do coordenador: fmt/check/cargo test (85 lib + 11 contract + 7 search) + test-db 54/54. Nota de tamanho: commit ~1962 linhas brutas (~880 produção fora de tests) — módulo coeso, import não quebra em dois commits sem quebrar spec≡router; registrado para o reviewer.
- **3e.3 FECHADO** (`44cf475`): UI completa pelo `@frontend-dev` — `lib/backup.ts` novo (export binário via blob + `Content-Disposition`; import com `replace` só quando true), `ImportDatasetModal.tsx` novo (glass-modal, 409 → fase de diálogo de irreversibilidade com arquivo preservado, toasts por `code`), DatasetMenu/galeria com Exportar habilitado, botão Importar removido do header de `/datasets` (P2). Build web verde; smoke Chrome com screenshots (download real `unzip -l` válido, import 201 com contagens + navegação, 409 → Cancelar/Substituir → 201 provado por API id antigo 404, console limpo). Dois achados pegos no próprio smoke e corrigidos: `required` nativo no file input quebrava o fluxo Cancelar→Importar (removido, validação em JS); id duplicado de a11y no modal. Nota para o reviewer: botão destrutivo "Substituir" em ROSA (paleta semântica da casa) — conferir contra design-system.md. Datasets de teste no ambiente para limpar: `backup-smoke` (ebf396c5), `backup-restaurado` (e063b448); `trigger` e `meu` intocados (não são da fatia — `meu` apareceu durante o smoke, provavelmente do usuário).
- **Em curso — 3e.4 (revisão)**: `@reviewer` despachado com o diff completo da fatia (`main..HEAD` código) contra a ADR-0006 — invariantes da casa + pontos de atenção registrados (tamanho do 3e.2 ~880 produção fora de tests; botão rosa; dívida ordem-de-boxes permanece).
- **Fila**: 3e.2 Import (SEQUENCIAL — mesmo módulo + openapi) → 3e.3 UI (paralelo SÓ se restrito a `apps/web/**`) → 3e.4 @reviewer → 3e.5 @docs-sync → verificação do coordenador com roundtrip E2E (§6 do plano: fidelidade por API).

### Sessão 9 — plano da 3e ANTECIPADO em arquivo (2026-09-07 — NADA implementado)

- Usuário pediu antecipação do planejamento, gravado em arquivo, sem implementar. Entregue: **`docs/plano-3e-export-import.md`** — escopo (fontes: IDEIA §1, PRODUCT.md :28/:41, ADR-0003 D9, backend.md :51/:249, frontend.md :96/:180), o que a fatia herda (ganchos de UI desabilitados, spec 0.5.0, `origin='import'` já no schema, sem migration esperada), decisões já travadas vs. **decisões abertas D1–D9 para a ADR-0006** (`@architect` no 3e.0), esqueleto de commits 3e.0–3e.5 com donos (3e.1/3e.2 SEQUENCIAIS — mesmo módulo), verificação (roundtrip export→import como critério de aceitação), **6 perguntas ao usuário a fechar antes da ADR** (inclui mover `POST /:id/package` para a fatia 4 — recomendação do coordenador) e checklist de retomada (§9 do plano).
- **Próximo passo quando a fatia abrir**: checklist §9 do plano → perguntas → 3e.0 (@architect) → aprovação → branch `feat/datasets-export-import`.

### Sessão 7 — fatia 3f implementada e MERGEADA pelo usuário (contexto)

- **CI pós-3g confirmado**: run 21 (main) e run 20 (feat/datasets-management) VERDES — 3g fechada definitivamente.
- **Fatia 3f "busca semântica" COMPLETA — 10 commits (`b046f56`..`a9080ff`) em `feat/semantic-search`**:
  - **3f.0** spike 5/5 na branch `spike/search-pgvector` (`e07ce2d` harness+matriz `spike/SEARCH-SPIKE.md`, `3717a66` appêndice na ADR-0004) — tag `pg16-trixie@sha256:c8483555…` (a `pg16` default é bookworm → collation mismatch quebra CREATE DATABASE; regla: bases glibc postgres↔pgvector nunca divergem), crate pinado `pgvector =0.4.1` (0.4.2 exige sqlx 0.9), HNSW 10k: build 1.76s/p50 0.317ms/top-1 20/20, embedder mock 17.9ms batch32, paridade do mock fixada (sha256-chain, rust×python bitwise), pg_dump/restore preserva tipo e valores.
  - **3f.1 `b046f56`** migration `0004_search.sql` (extension vector + image_embeddings + HNSW) + compose db→pg16-trixie + t0004 (38/38).
  - **3f.2 `02a8475`** EmbeddingPort + MockEmbedder (algoritmo do spike, golden bitwise) + HttpEmbedder (batch ≤32, timeouts 30s/5s) + AppState (embedder/embedding_model) + envs de boot (`EMBEDDING_BACKEND=mock|http`, `EMBEDDER_URL`, `EMBEDDING_MODEL`) + envs no compose + 6 units novos.
  - **3f.3a `40e52ae`** engine trainer-clip modo `serve` (mock hash ENGINE_MOCK=1 stdlib puro; real open_clip ViT-B-32 laion2b LAZY — torch só no ramo real; extras pyproject `[serve]`; paridade provada de novo no ferro).
  - **3f.3b `f8bd370`** Dockerfile do engine (python:3.12-slim pinado, sem pip install) + serviço compose `embedder` (ENGINE_MOCK=1, bind loopback-only 127.0.0.1:8090, healthcheck stdlib urllib, volume models; R3 provado: IPs LAN recusam) — container healthy com golden provado dentro dele.
  - **3f.4 `c33edeb`** indexação assíncrona (D4): `index_dataset_images` (advisory lock `heph_index:{id}` lock/unlock na mesma conn, pendentes por NOT EXISTS, chunks de 100, GETs paralelos semáforo 4, upsert ON CONFLICT, transação por chunk) + upload dispara spawn fire-and-forget + `POST …/search/index` (202 indexing|not_indexed) + `GET …/search/status` (derivação D5 LITERAL: 0 embeddings → not_indexed — decisão do coordenador; o implementador tinha invertido a partir de erro do prompt do coordenador) + spec 0.5.0 parcial.
  - **3f.5 `ec32979`** rotas de busca: `GET …/search?q&k&classId&split` (pós-filtro k*4 cap 400; classId não-uuid → 400 consciente; split train|val) + `POST …/search/by-image` (sem embedder; threshold) + erros `index_not_ready` 409 / `embedding_unavailable` 503 + spec 0.5.0 completa + contract (400/404 puros sem db) + 5 testes db.
  - **Review @reviewer back-end: CONDICIONAL** — 15 pontos exigidos com prova; **1 [MAIOR] provado por experimento psql**: indexedCount não filtrava `deleted_at IS NULL` → lixeira + imagem nova = `ready` FALSO (R4 invisível). **Fix `6fc3a06`**: JOIN com imagens ativas no count (status E 409) + teste de lock concorrente (2 spawns paralelos → 2/0, ON CONFLICT) + `.pop().expect` removido. 48/48 db.
  - **3f.6 `564c69d`** painel de busca na galeria (barra texto + badge status 4 estados com polling 2s + "Indexar agora"; modo resultados substitui a grade com score mono; "Buscar similares" no hover dos cards ativos; toasts 409/503/400-404; a11y aria-live) + `lib/search.ts` + tipos. Smoke Chrome com screenshots validadas pelo coordenador (busca texto, similares top-1 score 1.00, badge "Busca pronta" sozinho).
  - **Review @reviewer fechamento (UI+docs): BLOQUEIA** — **1 [MAIOR] provado por leitura**: polling nunca armado após "Indexar agora" (efeito não re-roda; badge congelava em "Indexando 0/0" — o smoke não exercitou o fluxo porque upload indexa sozinho). **Fix `a9080ff`**: `startSearchPolling()` reutilizável chamado pelo efeito E pelo handleTriggerIndex + AbortController próprio no polling (o [MENOR]) + toast honesto para 202 not_indexed + foco ring emerald no input. **PROVADO no ferro pelo coordenador**: dataset com embeddings deletados → "Indexar agora" → badge "Busca pronta" sem reload, console limpo.
  - **3f.7 `288dd7c`** docs-sync: backend.md (§1 embedder como exceção de topologia, §4 modo serve, §9 +4 rotas/nota 3f, §10 image_embeddings+índices, §11 compose pg16-trixie+embedder+envs), frontend.md (§5.2 painel, §10 contratos), dividas.md (digest db pago parcialmente + 2 dívidas novas: teste @gpu manual do CLIP real ~600MB; planner pgvector seq-scan ~10k — sem fatia).
- **E2E de API provado no produto** (binário reconstruído `docker compose build principal`): upload → status `ready 1/1` automático (spawn provado em produção-like), search text (score ∈ [-1,1]), by-image top-1 **score 1.0** (paridade mock Rust×container provada no produto), 400/409/404, delete 204.
- **Branch pronta**: `feat/semantic-search` (10 commits). **ORDEM DE MERGE: `spike/search-pgvector` PRIMEIRO** (appêndice da ADR-0004 vive nele — NIT do reviewer), depois `feat/semantic-search`. Push + CI + merge = decisão do usuário (regra: workflow dispara no push; mergear só com CI verde).
- **NITs residuais sem ação**: ADR-0004 D5 linha 57 ainda lista 503 no by-image (alinhar num docs futuro — junto do merge do spike); Content-Length sem teto no serve.py (aceito R3); dep `items.length` no efeito de status é INTENCIONAL (re-checa pós-upload); One CTA (Buscar vs Treinar) refutado pelo reviewer (Treinar está disabled).
- **Ambiente**: compose de pé (db pg16-trixie, seaweedfs, manager, principal RECONSTRUÍDO com código 3f, embedder novo healthy), dev server :3000, Chrome :9222. Datasets de teste deletados; "Trigger" pré-existente intato.
- **CI run 22 (merge spike→feat, `b869713`) FALHOU no rust — causa provada e fixada (`84ebb47`, aguardando push do usuário)**: o service `postgres` do ci.yml usava a `postgres:16` oficial (SEM a extensão vector) — a migration 0004 (`CREATE EXTENSION vector`, primeiro push da feat) falha no setup → **48/48 testes de banco falharam (0 passed)** — falha de SETUP global, não de testes individuais. O run 21 (main) passou porque a main ainda não tinha a migration. **Fix**: service do CI → `pgvector/pgvector:pg16-trixie@sha256:c8483555…` (mesma imagem do compose, digest pinado). **LIÇÃO DE PROCESSO (vale para toda fatia)**: migration que exige extensão/versão de banco = o service do ci.yml é atualizado NO MESMO commit (gap de boundary: o compose é do rust-dev mas o ci.yml é de CI — ninguém cobriu; o reviewer da 3f não auditava ci.yml porque o passo de banco já existia na main).
- **Fix multi-dispositivo (pedido do usuário 2026-09-07, commit `6afa67b` na feat/semantic-search)**: página aberta pelo IP LAN falhava nas imagens — a presigned nascia com host `localhost:8333` (default) E o SeaweedFS estava loopback-only (`127.0.0.1:8333`). Duas camadas corrigidas: (1) compose ganhou knob `SEAWEED_PUBLISH` (default seguro `127.0.0.1`, R1 mantido); (2) receita no `.env.example`. **Aplicado no ambiente**: `infra/.env` LOCAL (não commitado — o compose com `-f infra/compose.yaml` lê o .env do DIRETÓRIO DO COMPOSE, não da raiz) com `SEAWEED_PUBLISH=0.0.0.0` + `S3_PUBLIC_ENDPOINT_URL=http://10.15.10.3:8333`; seaweedfs+principal recriados. **Provado**: S3 responde por IP (403 sem auth), presigned nasce com host IP e baixa 200, Chrome em `http://10.15.10.3:3000` renderiza imagem (naturalWidth>0, console limpo). Cuidado R1: bucket na LAN com credenciais LOCAL-DEV — rotacione se sair da rede. Se o IP mudar (DHCP): atualizar `infra/.env` e `up -d principal`.
- **Próximo passo**: usuário revisa/pusha/mergea (spike → feat) → **fatia 3e (export/import)** → fatia 4. 🌱 graft na sessão: ~170k tokens poupados (soma dos packs).

### Sessão 6 — fatia 3g FEITA — CI rust falhou em fmt; fix `599fc46` commitado, re-push feito pelo usuário (contexto)

- **CI run 19 (`0c5a995`) FALHOU no job rust**: `cargo fmt --all --check` — os commits da 3g saíram sem fmt (handlers.rs/models.rs/datasets_db.rs). web e compose verdes. **Fix `599fc46` (chore(fmt))** commitado; 64+10+37 re-verificados pós-fmt; sem mudança de semântica. **LIÇÃO DE PROCESSO (vale para toda fatia com código rust): o checklist de verificação do coordenador passa a incluir `cargo fmt --all --check` — e os prompts aos implementadores rust-dev devem pedir fmt rodado antes de reportar pronto** (a lição da sessão 4 existia e não foi aplicada ao dispatch — falha do coordenador). Re-push da branch = usuário (ou push coordenador se autorizado).

- **Fatia 3g "gestão de amostras e classes" COMPLETA — 11 commits (`23cdd88`..`ebbc1d3`), review @reviewer APROVA** (após BLOQUEIA inicial com crítico provado e corrigido). Entregue: **3g.0** migration `0005_image_soft_delete.sql` (`images.deleted_at`, unique parcial `(dataset_id, filename) WHERE deleted_at IS NULL` substituindo o constraint único — re-upload de filename na lixeira nasce linha nova, índice da lixeira, contadores filtrando deleted; ON CONFLICT do upload ajustado junto — sem o `WHERE` o índice parcial não casa → 500; `t0005` no test-db); **3g.1** `PUT /:id/classes` reconciliação por id + **409 `classes_in_use`** (guard; openapi no MESMO commit — lição: contract tests exigem spec≡router a cada commit; classId preservado no rename provado E2E); **3g.2** lixeira: DELETE imagem = soft (204 sem sweep), restore (204 | 200 `{filename}` com rename `_restaurado` + `copy_object` na StoragePort + delete key antiga best-effort; 503 se copy falha — nada parcial), purge `DELETE /:id/trash` (sweep por prefixo de imagem), `?deleted=true` no list; **3g.3** invisibilidade D13 (detail/data/boxes/caption/autoTracked ignoram deletadas) + `trashCount` derivado (cast `::int` — count do Postgres é bigint) + openapi **0.4.0** + nota na ADR-0004 (3f recalibra → 0.5.0); **3g.4** `ClassesModal` (galeria + editor, 409 mantém modal, copy honesta — dataset sem classe deixou de ser beco sem saída); **3g.5** lixeira UI (hover trash + toast com **Desfazer** via `Toast.action` 6s, pills Ativas|Lixeira, restaurar com desambiguação de filename, esvaziar com confirmação permanente; bug pego no smoke: apiFetch 204 → undefined → TypeError no `res.filename` — fix `res ?? {}`); **3g.6** ADR-0005 formal + docs-sync (backend.md §9/§10 nota 3g, frontend.md §5.2/§5.3/§10, dividas: GC da lixeira nova + quitação do sync L3). **Review 1: BLOQUEIA** — crítico PROVADO com probe: fase 2 do dance de UNIQUE rodava antes do DELETE das removidas → remover classe com idx menor que o destino de um mantido (ex.: remover a 1ª) violava `UNIQUE(dataset_id, idx)` → 500 (o test de remoção livre só cobria a ÚLTIMA classe — o caso que não colide). **Fix `2f9be9a`**: ordem OBRIGATÓRIA guard(dentro da tx) → fase 1 tmp → **DELETE removidas** → fase 2 finais → INSERT; testes db dos 3 cenários (1ª/meio/última); **fix `9278697`** web: 404 stale → refetch silencioso + toast info (3 handlers); **`ebbc1d3`** docs com a ordem. **Review 2: APROVA** (1 nit de duplicação não-bloqueante). Verificação: 64 units + 10 contract + 37 db; smoke E2E API 18/18 (reconciliação E2E com classId preservado pós-rename, 409 em uso, re-upload `stored` com filename na lixeira, restore com rename `px_restaurado.png`, purge com contadores 1/0/0); smoke UI 7/7 (undo, presigned na lixeira, purge, console limpo). Container do principal RECONSTRUÍDO com a 3g (401 provado nas 4 rotas novas). Dívida nova: GC automático da lixeira (dividas.md).
- **Branch pronta**: `feat/datasets-management` — push + CI + merge = decisão do usuário (regra: workflow dispara no push; mergear só com CI verde).
- **Fix `fix/web-bbox-insecure` (`b390dda`) MERGEADO** pelo usuário com a 3d (`2303a39` merge fix + `9de7378` merge 3d; main verde). Causa 1 PROVADA em aba real (Chrome dele, `http://10.15.10.3:3000`): acesso por IP LAN HTTP = non-secure context → `crypto.randomUUID` é `undefined` → `TypeError` no `onUp` DEPOIS de `setDraft(null)` (caixa some, sem erro visível). Nos smokes dos agentes (localhost:3000 = secure) funcionava. Fix: **`apps/web/lib/id.ts::newId()`** (randomUUID se existe, senão UUID v4 via `getRandomValues` — disponível em qualquer contexto). **Causa 2**: dataset sem classes descartava o desenho em silêncio — bloqueado na origem com toast (agora com saída real via ClassesModal da 3g). Review `@reviewer`: **APROVA COM NITS**. Provas pós-fix por IP: caixa criada com coords exatas, autosave PUT 200, zero erros. `next-env.d.ts` regenerado pelo build foi restaurado (ruído do gerador — usuário o commitou em `560d3e7`).
- **Dois gaps de produto levantados pelo usuário na sessão 6, viraram a 3g (nada implementado ainda)**: (1) **classes pós-criação** — backend só aceita classes no `POST /api/datasets` (cap 200); tabela `classes` suporta mas NÃO há rota de CRUD/edição (§9), modal só na criação → dataset criado sem classes é órfão para sempre pela UI; copy "opcional" do modal e painel do editor ("crie-as no painel de criação") estão desonestos hoje; (2) **excluir imagem** — backend sem `DELETE /api/datasets/:id/images/:imageId` (só delete do dataset inteiro, sweep D7); protótipo não previa. Resolvidos no ADR-0005 v2 acima.

### Sessão 5 — fatia 3d FEITA, aguardando merge do usuário (contexto)

- **Fatia 3d (galeria `/datasets/[id]` + editor BBox) COMPLETA na branch `feat/datasets-gallery` — 8 commits (`61dc75b..f62c691`), pronto para merge do usuário** (após `fix/web/...`-style review APROVA COM NITS fechado). Entregue: **3d.1** T7 quitada (`autoTracked` derivado via `EXISTS` nas 3 queries de `handlers.rs`; `DatasetRow.auto_tracked`; teste de integração 3 casos; openapi descrição atualizada); **3d.2** galeria real (types/ImagePage/ImageDetail/upload multipart `files`/grade de thumbs presigned + chip `split` + load-more; 4 ações disabled com titles honestos — AutoLabel=futura, AutoTracker=4, Exportar=3e, Treinar=4); **3d.3/3d.4** editor BBox `/datasets/[id]/annotate/[imageId]` (sidebar 288px do protótipo, moldura 600px com aspect REAL + zoom 50–250, desenhar/mover/resize se-resize/pan/atalhos B,V,H,[1-9],Delete,Esc/clamp01 por update/autosave debounced 800ms + botão Salvar/beforeunload) com payload PUT total **preservando conf/origin/trackId** de caixas existentes (invariante T7) — `BoxInput` ganhou os 3 opcionais; **3d.5** docs (nota frontend.md + openapi + dividas T7 quitada).
- **Review @reviewer: APROVA COM NITS** — check mais crítico provado correto (`origin:"" é OMITIDO do payload` — `if (b.origin)` falsy + backend defaulta manual; autotracker sobrevive ao resave). Corrigidos antes do fechamento: **F1** desseleção pós-draw (.onClick só deseleciona em `select`), **F2** edição durante PUT em voo (contador de mutação + reagendamento — sem perda silenciosa de anotação), **F3** `glass-panel`→`glass-menu`, **F4/F5** openapi + dividas. **F6** nota de tamanho (~1340 linhas no total da fatia) registrada, sem ação.
- **Defeito extra pego no smoke E2E além do review (F1′ do coordenador, corrigido)**: PUT boxes é `DELETE`+`INSERT` → ids do backend nascem NOVOS a cada save; o eco do servidor em `handleSave` nulificava a seleção da caixa recém-desenhada ~800ms pós-draw (coords/moséca). Fix: reassociação da seleção por índice de payload (`current[i] ↔ res.boxes[i]`; ordem do RETURNING confirmada no rodapé do smoke). Provas: `selected:true` na caixa nova pós-autosave; coords `0.599/0.1/0.3/0.25` exatas; `origin=manual` para nova.
- **Smoke E2E API (container principal RECONSTRUÍDO com a imagem da branch — `docker compose build principal && up -d`; estava há ~10h em binário pré-3d.1)**: login 200 → create → upload 2 imgs stored (dims 64/48 sniffadas) → PUT 1 box autotracker → **`autoTracked=true`** ✓ → PUT 2 boxes → substituição total ✓, metadados preservados (autotracker/trackId 7/conf 0.9) ✓ → delete 204. `cargo test -p api-principal` 55+10 verde, `test-db.sh` 25/25 (novo teste incluído), `npm run build --workspace=web` verde no ferro (5 rotas), smoke visual Chrome: galeria (header, meta com source `s3://…`, resumo, thumbs carregadas, chip train, tile dashed), editor (sidebar completa, zoom, desenho sintético + autosave + coords + ring). Minutíssimo achado ORM registrado em `dividas.md` (ordem de boxes — sem fatia).
- **Ambiente após a sessão**: compose de pé (db, seaweedfs healthy, manager, principal — agora com código 3d), dev server :3000. Dataset de teste deletado (204); dataset "Trigger" pré-existente deixado intato (não é meu; usuário decide). Nota de fermentation: `docker compose build principal` reconstrói da branch atual — quando o usuário trocar de branch, revisitar se o binário divergir do tronco.
- **Próximo passo**: usuário revisa/mergea `feat/datasets-gallery` em `main` (regra CI: workflow dispara no push da branch — pusha, acompanha via API, mergeia só quando verde; main sempre verde). Depois **fatia 3f (ADR-0004 busca semântica)** — 3f.0 spike pgvector primeiro; 3f.1–3f.5 podem ser despachadas em paralelo após a 3d mergeada; 3f.6 (UI) consome a galeria desta fatia. `@infra-dev` e novo roster entram nas próximas sessões (efetivo da contratação).
- 🌱 Economia graft nesta sessão: `find_code` (2 chamadas) ≈ 3.681 tokens poupados.

### Sessão 4 — ADR-0004 busca semântica + CI (contexto)

- **Contratação do `@infra-dev` (2026-09-05, pedido do usuário)** — dono mecânico de infra: `infra/` (compose), Dockerfiles, `scripts/` de verificação, CI quando spec pedir, `.env.example`. **Não decide arquitetura** (ADR vem do fluxo normal); migrations seguem com `@rust-dev`; não toca código de negócio. Permissões negadas: commits/push/merge/rebase (padrão) + `compose down`/`prune`/`rm` de volume/network/container (proteção do ambiente de dev de pé — a lição do `fix/infra-env` virou política). `variant: medium` (decisão do coordenador: blast radius de ambiente inteiro + falha silenciosa — mesmo rationale do `@fixer`; usuário pode rebaixar para `low`). **Efetivo na próxima sessão** (config não retroage em sessão viva — roster do Task tool é fixado no boot). Gap que motivou: sem CI (`.github/workflows` não existe), dívida de digests pendente, e a 3f adiciona trabalho de infra (imagem pgvector, serviço embedder). Charter em `.opencode/agent/infra-dev.md`.
- **Decisões de gestão do usuário (2026-09-05, fechamento da sessão 4)**:
  1. `chore/infra-agent` MERGEADA pelo usuário (`662fb4d`) — `@infra-dev` efetivo na
     próxima sessão.
  2. **Digests: APROVADO e FEITO** — commit `4b8c7e4` em `chore/pin-digests`
     (despacho `@rust-dev`; dívida QUITADA em `dividas.md`; aguardando merge). Nota
     nova registrada: manager/orchestrator ainda em `bookworm-slim` — migrar para
     `trixie-slim` quando o orquestrador ganhar cliente S3 (fatia 4, R10).
  3. **CI = Gitea Actions (CORREÇÃO — registro anterior errado dizia "descartado")**:     o usuário se auto-hospeda em `git.felipecncloud.com` (origin) e estava
     configurando o **gitea-runner** quando perguntei; workflows em `.gitea/workflows/`
     (formato GitHub-compatível do act_runner). Desenho v1 acordado verbalmente (sem
     arquivo ainda): job rust (`cargo fmt --all --check`, `cargo check --workspace`,
     `cargo test -p api-principal` — sem banco), job web (`npm ci` + build), job compose
     (`config -q` — não precisa de daemon). V2: testes de db com `services: postgres`;
     storage tests só com docker-in-docker (adiar); engines Python entram no CI na 3f.3.
     **Imagens + digests para o runner mirar (resolvidos 2026-09-05, registry oficial;
     digests preservam-se ao copiar para o registry do Gitea)**:
     `rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c`,
     `node:20-slim@sha256:2cf067cfed83d5ea958367df9f966191a942351a2df77d6f0193e162b5febfc0`,
     `docker:29-cli@sha256:eccaacfeed644c7de222ff047483568cb988dde95476fbaaf10ea2d04921bb66` (29 = major do docker do host 29.7.2),
     `postgres:16@sha256:f1c3376c26f2609ab9f29f71f824103fe2fcd8ee0346485cb6122a4f93df6f94` (v2),
     `python:3.12-slim@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea` (3f.3).
     Pendências para escrever o `ci.yml`: label do runner (`runs-on:`) e se ele alcança
     o Docker Hub (senão, mirar via registry do Gitea).
     **Estado da iteração de CI (2026-09-06, monitorada pela API de Actions com token
     read-only do usuário em `~/.config/hep-ci/token` — FORA do repo)**: run 1
     (`079eb88`) provou digests ✓ + healthcheck do service ✓ + download de action ✓ e
     derrubou `actions/checkout` (act_runner v3.3.2 NÃO injeta node em actions JS —
     exit 127 em imagens sem node). Fix (`22c6f76`, mergeado): checkout manual
     `git clone` + token automático via header basic `x-access-token` (esquema do
     actions/checkout). Run 2 (`d05d7b9`): job `compose` VERDE de ponta a ponta
     (prova do caminho inteiro); `rust` falhou por `rustfmt` ausente no
     `rust:1.97.1-slim` (perfil mínimo do rustup) e `web` por git ausente no
     `node:20-slim` (base debian-slim puro, NÃO scm-slim — errata do coordenador).
     Fix no run 3: `chore/ci-round3` (`c224ca6`) — `rustup component add rustfmt` +
     `apt-get install git` no web. **Regra de processo (pedida pelo usuário, vale
     para sempre): NUNCA mergear na main para testar CI — o workflow dispara em
     `on: push` de QUALQUER branch; pusha a branch, acompanha via API, mergeia só
     quando verde. Main deve estar sempre verde. Recomendação ao usuário: ativar
     branch protection em `main` (Settings → Branch → Enable Status Check exigindo
     `rust`/`web`/`compose`) para o gate virar mecânico.** Acesso do coordenador à
     API: `GET /api/v1/repos/Felipe/Hephaestus-LLM-Studio/actions/runs` e
     `…/actions/jobs/{id}/logs` (o endpoint `…/tasks/{id}` individual não existe no
     1.27.1 — usar o `jobs` do run).
  4. **Push do tronco: feito pelo usuário** (após o merge do infra-agent; o ADR-0004
     e o pin-digests/housekeeping ainda não estão no origin).
  5. **`cargo fmt`: APROVADO e FEITO** — commit `8947fec` em `chore/housekeeping`
     (`cargo fmt --all`, 14 arquivos; `cargo check --workspace` + `cargo test -p
     api-principal` verdes; dívida do fmt quitada com o merge). Lição operacional:
     o type-enum do commitlint é `[feat, fix, docs, refactor, test, chore]` — `style`
     NÃO existe; usar `chore(fmt)`.
  6. **3d na próxima sessão**: usuário abrirá sessão nova e fará levantamento leve
     se a 3f afeta o que a 3d vai fazer (resposta: a 3f não muda o desenho da 3d —
     só consome a galeria que ela cria; ver ADR-0004 D0).
- **ADR-0004 ACEITA pelo usuário (2026-09-05): busca semântica sobre datasets com embeddings OpenCLIP = fatia 3f**, especificação completa em **`docs/adr/0004-semantic-search.md`** (D0–D8, migration `0004`, spike `3f.0` com 5 critérios binários, plano de commits 3f.0–3f.7). Resumo das decisões: pgvector no Postgres existente (compose troca `postgres:16` → `pgvector/pgvector:pg16` com digest pinado — spike prova upgrade sem dump/restore, R1); embedder = `trainer-clip` em modo `serve` como serviço compose (fora do orquestrador até a fatia 4 — exceção consciente à topologia, com caminho de unificação); indexação assíncrona SEM fila (estado derivado `indexedCount` vs `imagesCount` + advisory lock — não depende da fatia 4); 4 rotas novas, spec 0.4.0, erros novos `index_not_ready` (409) e `embedding_unavailable` (503); EmbeddingPort com `MockEmbedder` default (`EMBEDDING_BACKEND=mock`). Dedup e AutoLabel assistido fora de escopo v1 (schema não fecha portas). **Nada implementado** — docs de contrato só mudam no commit 3f.7 (lista de linhas que ficam falsas está no fim da ADR).
- **Sequência do roadmap atualizada**: 3d → **3f** → 3e → 4. A 3f depende apenas da 3d (a busca mora na galeria); 3f.1–3f.5 são disjuntos de 3e/4 e podem ser despachados em paralelo ao fim da 3d; só 3f.6 (UI) espera a galeria.

### Sessão 3 — alinhamento de design + fixes de ambiente (contexto)

- **Problema reportado pelo usuário**: implementadores frontend desviando do estilo de layout (impeccable/OpenDesign como referência; troca de modelo do dev + protótipo regenerado com login no OpenDesign como teste). Fechado em 3 frentes:
  1. **Referência formal** — `docs/design-system.md` MESCLADO (base OpenDesign: frontmatter YAML navegável + Do's/Don'ts + regras nomeadas; seções exclusivas da versão impeccable reincorporadas: iconografia, anatomia de componentes, a11y, avaliação crítica; token Runtime Python `#eab308` recuperado com prova v1:864). **Descoberta do reviewer**: o `ai-vision-training-studio.html` do tronco JÁ É a regeneração OpenDesign (3641 linhas, `LoginPage` ~329) desde o merge `chore/opendesign` (`39a1410`) — o `ai-vision-training-studio-v2.html` que o coordenador importou do OpenDesign era byte-idêntico e foi REMOVIDO (`1a00b22`); referência única = protótipo da raiz.
  2. **Auditoria+correção visual** (`@ui-designer` qwen3.7-plus, Chrome flatpak :9222 + skill chrome-mcp): `/login` (blobs de luz zenital, meta v1.3, placeholder, autoFocus, focus ring emerald, footer "Single-User Mode" SEM botão demo — instrumentação rejeitada); DatasetCard (violet/sky → `text-zinc-300` PROVADO contra v1:1528; p-5; tiles com borda; chips estilo v1; "Treinar →" text-link); TabsBar (aba ativa `text-white` sem underline); Topbar (inline style → classes; superfície `zinc-950/80` MEDIDA E PROVADA igual à v1 — suspeita inicial do coordenador era falsa); modal `max-w-lg`. Pendências fechadas via `@frontend-dev`: `IconLock` (padrão Base, paths do protótipo) + ícones no DatasetMenu (adaptações declaradas: Eye→IconLayers, Play→IconTarget). **Smoke do login com senha dev `changeme`: 4/4 PASS** (autoFocus; POST 200 + cookie; redirect; 401 → "Senha incorreta."; regression redirect; console limpo).
  3. **Causa-raiz** — charters de `@frontend-dev`/`@ui-designer` agora apontam `docs/design-system.md` como fonte de ESTILO (paleta FECHADA, regras nomeadas: One CTA/Monospace Truth/Refractive Edge/Class Palette Integrity; cores fora da paleta proibidas) e o protótipo como fonte de LAYOUT. É o mecanismo anti-improviso para os implementadores low.
4. **Organização — dívidas viraram registro próprio (inspiração: artigo "Harness
   Engineering" da OpenAI, 2026-02-11)**: seção de dívidas extraída deste arquivo
   para **`docs/dividas.md`** (registro permanente: em aberto/quitado, fatia marcada,
   como atualizar). Este arquivo referencia e não duplica; pendências do Fecho
   também migraram para lá.
- **Fixes de ambiente (desbloqueio do dev; branch `fix/infra-env` `c09569a`)**: healthcheck do SeaweedFS dependia de GNU wget (exit 8 em 403); a tag **mutável** `4.45_full` trocou o wget para BusyBox (exit 1) → container unhealthy permanente → novo probe portável (aceita QUALQUER resposta HTTP: `wget -S … | grep -q HTTP/1.1`). Runtime do principal `bookworm`(glibc 2.36) → `trixie-slim` (builder rust:slim é trixie/2.41; `aws-lc-sys` exige GLIBC_2.38 — **R10 da ADR-0003 materializado por tag mutável**). **Lição: tags de imagem mutáveis quebram builds verificados; recomendação PENDENTE ao usuário: fixar digests no compose/Dockerfiles.**
- **Review da `fix/web-design-alignment`: APROVA** — 1 menor corrigido (v2 duplicado removido) e 1 menor REFUTADO com prova empírica: botão "Treinar" disabled — a regra global `button:disabled` do globals.css JÁ aplica opacity .55 + not-allowed (getComputedStyle confirmado) e o hit-test resolve no próprio botão (sem click-through ao Link) — falso positivo duplo do reviewer, registrado como lição (provar antes de corrigir).
- **Branches aguardando MERGE do usuário (ordem importa)**: ~~todas~~ **MERGEADAS em 2026-09-05**: `fix/web-3c-review` (`0554d3f`, pelo usuário), `fix/web-design-alignment` (`a19d835` — conflito em TabsBar resolvido pelo coordenador: 6 abas da emenda + decisão visual da auditoria na aba ativa `text-white` sem underline; DatasetCard/Modal auto-mergeados, tag AutoTracker preservada, `max-w-lg` combinado), `fix/infra-env` (`fdfaff6`), `chore/agent-design-ref` (`11ebdfb`). **Verificação pós-merge**: build web verde (rota `ƒ /datasets/[id]` viva), compose config OK, smoke visual no Chrome: 6 abas + badge de contagem, aba ativa `rgb(255,255,255)` sem underline, card p-5/16px com Refractive Edge provado (borda topo `0.13` vs laterais `0.07`), botão Treinar com affordance global (opacity .55 + not-allowed). Branches de fatia ainda não apagadas — decisão do usuário. `main` ~6 à frente do origin (push pendente).
- **Nota docs**: `docs/frontend.md` linha 3 ainda descreve o protótipo como "~2910 linhas" — o do tronco é a regeneração (3641, com LoginPage); sincronizar no próximo docs-sync.
- **Ambiente de dev de pé** (não desligado): compose (db, seaweedfs healthy, manager, principal :8080 — senha dev `changeme`), dev server Next :3000, Chrome :9222 (flatpak).

### Sessão 2 — 3b mergeada, 3c revisada e emendada (contexto)

- **Fatia 3b MERGEADA no tronco pelo usuário** (`04b8987 Merge branch 'feat/datasets-storage'`) — storage S3/SeaweedFS fechado, spec 0.3.0.
- **Fatia 3c (UI `/datasets`) NO TRONCO via `1f182ed`** — 3 commits (`cf2b978` fundação do shell: tipos, api lib, format, icons, Topbar/TabsBar/Toast; `f8bca9e` lista grade+tabela com filtros e empty states; `ce29fa4` criar/excluir com modal, confirmação, menu de contexto e toasts). O mesmo merge trouxe `d6e4e1d` (troca de modelos dos agentes — config puro, conferido pelo coordenador).
- **Revisão da 3c: CONDICIONAL** (`@reviewer`, despacho único): contrato/casing/fetch 1:1 com openapi 0.3.0, zero críticos. Condições F1–F6 (link de galeria → 404; só 1 das 6 abas do shell, sem badge; code `"validation"` morto fora do enum; coluna Ações ausente na tabela; `trainTabFor` dead export; tag AutoTracker ausente). **Emenda implementada via `@frontend-dev` e verificada pelo coordenador** (build web limpo com rota `ƒ /datasets/[id]`; greps `validation`/`trainTabFor` zerados) em **branch `fix/web-3c-review`** (`f7889b2` fix(web) + `41739c2` chore gitignore) — **aguardando merge do usuário**. F7 (sem teste de UI) não bloqueia = backlog §12 do frontend.md.
- **`main` está 5 commits à frente de `origin/main`** e `fix/web-3c-review` soma 2 — push/merge = decisão do usuário.

### Histórico das sessões anteriores (contexto)

- **Fatia 3b LANDED na branch `feat/datasets-storage` (3b.0–3b.7:
  `f6c6ff5`..`393163c`) + docs sincronizados (3b.8, working tree desta sessão, sem
  commit — o coordenador commiteia). Branch à frente de `main`; merge = decisão do
  usuário. Entregue: migration 0003 + `StoragePort`/`MockStorage`/`S3Storage` + 6 rotas
  (upload, images, detail, `/data`, boxes, caption) + sweep pós-commit + `source`
  derivado + `classes{id}`; spec 0.3.0; revisões 3b.3/3b.6 feitas. Dívida 3b QUITADA
   (ver `docs/dividas.md`); sobraram: logging server-side (fatia nomeada), gate `sub` órfão,
  `cargo fmt`. A/B 3b.6 registrado no bullet do experimento — **encerrado pelo usuário:
  sem swap; `@reviewer` é o despacho único, max só como escalada** (`a343a7c` em
  `chore/reviewer-escalacao`).

- **Spike 3b.0 EXECUTADO e PASSOU (7/7).** Rodado no ramo **descartável**
  `spike/storage-seaweedfs` (commit `09a517d`; **fundido pelo usuário em `main` (`52da6f9`)**
  — matriz `spike/STORAGE-SPIKE.md` e harnesses vivem no tronco). Consequência: **D4 (crate) e D3 (presigned)
  ficam aprovadas, sem inversão**; R2 e R3 desriscados no ferro. Os achados que **corrigem o
  rascunho da ADR** (identidade via `-s3.config` JSON e não env vars; bucket auto-cria sem
  init-container; healthcheck exige `-ip.bind=0.0.0.0`; nomes reais da API do SDK; novo risco R10
  = build do `aws-lc-sys` no `rust:slim`) estão appêndados na **ADR-0003**, seção "Resultados do
  spike 3b.0" — **ler antes de codar a 3b.4**. `main` limpa de worktree; **1 commit à
  frente do `origin/main`** (`f4d1551`), push pendente = decisão do usuário.
- **Branch de trabalho: `main`.** `feat/datasets-core` foi **mergeada pelo usuário**
  (`e724436 Merge branch 'feat/datasets-core'`) e as branches de fatia foram apagadas,
  incluindo a de segurança `backup/pre-reword-3a` (confirmado antes de apagar: árvores de
  código byte-idênticas aos commits que entraram; o único resíduo era o hash pré-reword de
  um commit cujo conteúdo é o mesmo). Situação de push: ver bullet acima (`main` à frente
  do origin em `f4d1551`).
- Roadmap `docs/repo-estrutura.md` §Ordem: Slice 1 ✅, Slice 2 ✅, **Slice 3a ✅ (no
  tronco)**, 3b é o próximo passo.
- **Slice 3a no tronco**: `GET/POST /api/datasets` + `GET/DELETE /api/datasets/:id` com
  migration `0002` (`datasets`+`classes`), primeira rota de negócio → gate
  `route_layer(require_auth)` plugado (dívida do ADR-0001 D9 quitada), OpenAPI
  0.2.0, ADR-0002 escrita. Verificação: `cargo check --workspace` limpo,
  `cargo test -p api-principal` = 27 units + 7 contract verdes sem banco,
  `bash scripts/test-db.sh` = 7 integration verdes com Postgres do compose,
  `compose -f compose.yaml -f compose.integ.yaml config -q` OK.
- **ADR-0003 (storage de objetos) ACEITA pelo usuário, servidor = SeaweedFS.** Nada
  implementado ainda; é a próxima fatia. Ver "Próximo passo" abaixo e
  `docs/adr/0003-object-storage-s3.md`.
- **Decisão estrutural da 3a (ADR-0002 D1)**: casing no wire é **camelCase em
  `/api/*` inteiro**; colunas SQL, valores de enum, `Error.code` e artefatos de
  transporte (`manifest.json`, `config.yaml`, SQLite) ficam **snake_case**.
  Enforcement por teste (`json_property_names_are_camel_case`, walk recursivo).
  Isso altera o que os docs de settings exemplificavam → `hf_token` virou
  `hfToken` no wire em `backend.md` §9 e `frontend.md` §10 (rota ainda não
  existe). Não regrida isso por acaso.
- `apps/web` continua com só `/` e `/login`; `/datasets` (3c) ainda não existe.
- Ferramental: `@ui-designer` despacha (exige dev server + Chrome :9222).
  Grafo graft em dia (`graft/` é git-ignored — não se commite); 2 nós de
  `layout.tsx` seguem pendentes no meaning tier (modelo local falha lá, cosmético).
- **Cadeia operacional atualizada (2026-09-05, `chore/agent-team`, pendente de merge):**
  gate de commit vivo (`lefthook.yml` → commitlint no `commit-msg` + aviso de
  staging >400 linhas e bloqueio de segredos/`target/` no `pre-commit`; setup novo:
  `npm install && npx lefthook install`); subagentes com `git commit/push/merge/rebase`
  **negados em runtime** (só o coordenador commiteia); `graft` mandatório em
  `fixer`/`reviewer`/`docs-sync`; entregável do `architect` = formato ADR + plano de
  commits numerado; checklist do `reviewer` com invariantes da casa (casing D1,
  body-limit, ordem objeto→linha→compensação, contadores por função única);
  `ui-designer`/`frontend-dev` alinhados ao Tailwind v4 com desempate de posse;
  skill `hephaestus-dev` regrava (um dispatch = um commit, todo por passo com
  atualização em tempo real, spikes = coordenador com loop de build em script único
  — lição do 3b.0). Anti-exemplo registrado na skill: `1c1f72f` (3.219 linhas em 1
  commit) que virou a cirurgia de reword da 3a.
- **Experimento A/B de revisor (3b) — ENCERRADO pelo usuário em 2026-09-05** (custo de
  tokens; decisão registrada ao fim da fatia). `@reviewer-max` (qwen3.8-max, `variant: high`,
  corpo idêntico ao titular) despachou no MESMO diff que o `@reviewer` nos marcos
  3b.3 e 3b.6. **Veredito do usuário: sem swap — `@reviewer` (flash/high) é o despacho
  único de marco; `@reviewer-max` fica no time como ESCALADA** (só quando o titular não
  resolver, travar no mesmo ponto, ou risco alto pedir auditoria independente — charter
  reescrito em `a343a7c`/`chore/reviewer-escalacao`). *Dia 1 (smoke em `86fb0eb`)*: ambos
  BLOQUEIA no mesmo defeito real (gate de segredos × `!.env.example` do gitignore —
  comprovado por matriz de 4 casos antes do fix); o titular ainda cruzou com a ADR-0003
  (`.env.example` é entregável prometido da 3b.4) — 1 ponto pro flash. *Marco 3b.3
  (difícil, teste real)*: ambos **BLOQUEIA** pelo MESMO crítico comprovado por sonda
  própria (livelock multipart pós-`LengthLimit` — axum embrulha corpo todo, multer nunca
  fuseja; os dois citaram a fonte e reproduziram), **zero falso positivo nos dois lados**.
  Titular achou a mais: duplicate falso por stem sem extensão (F4) e a janela de boot mock
  (F6, decisão do coordenador); sombra achou a mais: doc de `sanitize_filename` mentindo +
  branch morto (F7) e o staleness latente do `COALESCE(NEW,OLD)` em UPDATE de
  reparentização (registro: inofensivo até existir rota de UPDATE de `image_id`/`dataset_id`).
  Contagem: 2 titular × 2 sombra — empate técnico. *Marco 3b.6*: titular CONDICIONAL com
  **1 falso positivo** (emenda vista só no trunk — o diff do marco não continha o fix) e 2
  únicos (erros por-chave do `delete_prefix`, TTL não wired no compose); sombra PASSA com 2
  únicos (órfão `infra_pgdata` no runner, upsert com `RETURNING`) e 0 falsos. **Placar
  final: 3b.3 empate 2×2; 3b.6 2×2 com vantagem da sombra só em falsos positivos.**
  Conclusão operacional: achados convergentes nos dois marcos — **o segundo despacho nunca
  mudou um desfecho que o titular + coordenador não tivessem alcançado; o duplo despacho
  não se paga.** Hipótese "medium no fixer reduz escaladas" segue válida (é outra linha do
  experimento, sem custo de modelo caro).
- **Esforço de razonamento fixado por agente** (`variant:` na frontmatter, validado
  no provider): **coordenador `@hephaestus` sobe para qwen3.8-max/`medium`** (decisão do
  usuário 2026-09-05, informed pelo A/B: o loop redundante de decisão em flash/high custou
  mais que o differential do modelo — max decide certo com cadeia menor); `high` em
  architect/reviewer; escalada `@reviewer-max` só quando o titular travar; `medium` em
  fixer e ui-designer; `low` nos implementadores e explore. **Novo @visao**
  (flash/`low`, permissões edit/bash/web negadas): proxy de visão do coordenador —
  transcreve screenshots/PNGs de forma fática quando o usuário anexa imagem; NÃO audita
  tela (isso segue sendo do `@ui-designer` com DevTools: DOM+computed styles+edição, que
  "descrever pixels" não substitui). Fallbacks de uma linha: se max/medium mostrar
  verbosidade ou loop novo no coordenador, testar `low`, e rebaixar para flash/high é o
  último passo; a medição natural é a sessão da 3c. Hipótese "medium no fixer reduz
  escaladas" segue válida. Vale a partir da próxima sessão (config não retroage em sessão
  viva; em `chore/reviewer-escalacao` `bc4d5db`).

## Storage da 3b — decisão TOMADA (2026-09-04): bucket S3/SeaweedFS

Origem: o usuário propôs **MinIO** (como ele já opera no VisionLens,
`/home/felipecn/DEV/VisionLens`) para imagens canônicas e labels/coordenadas no banco.
O `@architect` desenhou e eu aprovei a direção; **o usuário aceitou a ADR-0003 e escolheu
SeaweedFS** como servidor. Tudo está em
**`docs/adr/0003-object-storage-s3.md`** — ela é a especificação da 3b, leia antes de
codar. Resumo do que já está fechado lá:

- **D1** bucket = único blob canônico; disco local só efêmero (árvore YOLO/`.txt`/
  `data.yaml` nasce em tempdir **no orquestrador** e morre no `finally` — padrão validado
  no `yolo_trainer.py` do VisionLens).
- **D2** upload **via principal** com **spool em tempfile + `put_object` com
  content-length exato.** Nunca stream de tamanho desconhecido (trilha
  `aws-chunked`/`STREAMING-*-TRAILER`), nunca presigned browser→bucket (exigiria
  `complete`+`HEAD`+sonda ou a máquina de notificação de bucket + estado "pendente").
- **D3** leitura híbrida por `S3_PUBLIC_ENDPOINT_URL` (presigned **assinado no host
  público** — gotcha SigV4 do header `Host`, lição do VisionLens) com fallback
  `GET …/images/:imageId/data` **sempre disponível**.
- **D4** `aws-sdk-s3` **sem** `aws-config`, `force_path_style(true)`,
  `request_checksum_calculation(WhenRequired)`. Nenhum `reqwest` na 3b.
- **D5** chaves legíveis `datasets/{dataset_id}/images/{image_id}/{filename}`;
  `images.path`→`object_key`; **`datasets.source` sai do banco** (DROP na 0003) e vira
  derivado no wire `s3://{bucket}/datasets/{id}/` quando `images_count > 0`.
- **D6/D7** só mídia vira objeto; ordem **objeto→linha→compensação** e sweep de prefixo
  **pós-commit** no `DELETE /:id`.
- **D8** `src/storage/{port,mock,s3,keys,sniff}.rs` + `MockStorage` (testes sem rede);
  **manager e orquestrador não têm cliente S3 na 3b**.
- **D9** `export`/`import`/`package` **sairam da 3b e viraram fatia 3e** (backup interim =
  UI do servidor + `mc mirror`); o chunking de 8 MB sobrevive só no transporte
  principal→orquestrador **remoto**.
- **D10** um erro novo só: `storage_unavailable` (503). Spec 0.2.0 → **0.3.0**.

Por que não MinIO (e-a-confirma-na-fonte, não é palpite): repo `minio/minio` **arquivado
pelo dono em 25/04/2026** ("THIS REPOSITORY IS NO LONGER MAINTAINED", só código-fonte,
sem binário de comunidade; última release out/2025) e a **GHSA-9c4q-hq6p-c237 /
CVE-2026-40344** (bypass de assinatura na trilha `STREAMING-UNSIGNED-PAYLOAD-TRAILER`) com
**"Patched versions: None"** no OSS. As issues #21611 e **#21303 (esta é o SDK Rust com
`ByteStream::from_path`)** documentam a trilha de streaming quebrada que a D2 evita.

**Nenhum doc de `backend.md`/`frontend.md` foi alterado** — a lista de linhas que ficam
falsas está no fim da ADR-0003, marcada para o commit `3b.8` (docs descrevem o que existe,
não o que foi aprovado).

## Dívidas técnicas

As dívidas técnicas e pendências vivem em **`docs/dividas.md`** — registro
permanente de primeira classe (em aberto / quitado, com fatia marcada quando
aplicável; inspirado no `tech-debt-tracker` do artigo "Harness Engineering"
da OpenAI). Este arquivo não as duplica: ao registrar, marcar fatia ou quitar
uma dívida, atualize o `dividas.md`. Fatias novas DEVEM ler o `dividas.md`
(item 4 do protocolo de retomada) e honrar as dívidas relevantes no
nascedouro.

## Plano em andamento — PRÓXIMO PASSO EXATO: fatia 3e (export/import) — ADR-0006 (3e.0)

**Sequência daqui:** **3e export/import (ESTA FATIA — em curso)** → **4 jobs/package/materialização** (orquestrador ganha cliente S3 com credencial escopada por prefixo e absorve o embedder como runner-CLIP — mesma interface HTTP da 3f; dívida T4 da ADR-0002 — `jobs.dataset_id ON DELETE SET NULL` + `dataset_versions` — honrada no nascedouro; **herda `POST /:id/package` movido da 3e por decisão do usuário 2026-09-07**). 3h = definição a confirmar com o usuário/roadmap. Estado detalhado da 3e no topo (Sessão 10) e em `docs/plano-3e-export-import.md`.

**3b.0–3b.8 FEITOS e MERGEADOS (`04b8987`); 3c FEITA, REVISADA e EMENDADA (ver "Estado atual").** A sequência:

1. ~~`spike/storage-seaweedfs`~~ ✅ **CONCLUÍDO 2026-09-05** (ramo `spike/storage-seaweedfs`,
   commit `09a517d`, matriz em `spike/STORAGE-SPIKE.md`; **fundido pelo usuário em `main` (`52da6f9`)** — harnesses e matriz vivem no tronco).
2. ~~**3b.1..3b.7**~~ ✅ **CONCLUÍDOS** em `feat/datasets-storage` (`f6c6ff5`..`393163c`);
   `@reviewer` ao fim de 3b.3 e 3b.6 (ver A/B no "Estado atual").
3. ~~**3b.8**~~ ✅ **CONCLUÍDO nesta sessão** (`@docs-sync`: deltas da ADR-0003 aplicados em
   `backend.md`/`frontend.md` + banner na ADR-0002 + ADR-0003 marcada IMPLEMENTADA).
4. ~~3c UI `/datasets`~~ ✅ **NO TRONCO** (`1f182ed`, 3 commits) + revisão CONDICIONAL
   fechada com emenda em **`fix/web-3c-review`** (`f7889b2`, `41739c2`). Próximo:
   **usuário mergeia `fix/web-3c-review`** → **3d galeria `/datasets/[id]` + editor
   BBox** (o placeholder honesto criado pela emenda é substituído pelo conteúdo real;
   upload UI entra aqui) → **3e export/import** → **4 jobs/package/materialização**
   (onde o orquestrador ganha cliente S3 com credencial escopada por prefixo e onde a
   dívida T4 da ADR-0002 — `jobs.dataset_id ON DELETE SET NULL` + `dataset_versions` —
   precisa ser honrada no nascedouro).

Cada fatia: branch `feat/<slice>` de `main` atualizada, commit `type(scope):
subject`, verificação do coordenador (`cargo check --workspace`, `cargo test -p
api-principal`, `bash scripts/test-db.sh` quando houver teste de banco, `bash
scripts/test-storage.sh` quando houver storage, `npm run build --workspace=web` quando
houver UI, `compose config -q`), sem push/merge sem pedido. Commits **fora de
`main`** (a regra da casa; `docs/coordenacao.md` e ADRs são as exceções que o usuário já
autorizou a landing direto no tronco).

## Fecho

- [x] Fatia 3a mergeada em `main` pelo usuário (`e724436`) e branches de fatia apagadas.
- [x] ADR-0003 aceita, D0 = SeaweedFS.
- [x] Spike `spike/storage-seaweedfs` rodado (7/7 PASS, commit `09a517d`; **fundido pelo
      usuário em `main` `52da6f9`**); achados appêndados na ADR-0003 → "Resultados do spike 3b.0".
- [x] **3b.7** ✅ (sweep + `source` derivado + classes com `id`, `393163c`) e **3b.8** ✅
      (docs sincronizados nesta sessão).
- [x] **3b mergeada** (`04b8987`); **3c no tronco** (`1f182ed`), revisada (CONDICIONAL,
      F1–F6) e emendada em `fix/web-3c-review` — build web limpo, greps zerados.
- [x] Próximo passo = ~~4 merges~~ ✅ **MERGEADOS** (3c-review pelo usuário `0554d3f`; design-alignment `a19d835` com conflito de TabsBar resolvido; infra-env `fdfaff6`; agent-design-ref `11ebdfb`) → **3d é a próxima fatia**.
- [ ] Pendências e dívidas que continuam valendo: ver **`docs/dividas.md`**
      (registro permanente — inclui CLI `reset-password`, `cargo fmt`,
      logging server-side, gate `sub` órfão, digests de imagem, testes de UI,
      sync `docs/frontend.md` linha 3).
      ~~`lefthook install`~~ ✅ quitado em `chore/agent-team` (gate ativo: hooks
      instalados + commitlint real + deny de commit nos subagentes).
