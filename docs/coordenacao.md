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
   `docs/frontend.md` §10, `docs/repo-estrutura.md` (ordem de fatias) e os ADRs em
   `docs/adr/` — a 3b tem especificação própria e completa em
   **`docs/adr/0003-object-storage-s3.md`** (decisões D0–D10, delta de contrato,
   contorno da migration 0003, plano de commits 3b.0–3b.8); não reinvente nada que já
   está lá, e não aplique os deltas de `backend.md`/`frontend.md` antes do commit 3b.8.

## Estado atual — 2026-09-05 (fim de sessão da 3b; retomada = ler esta seção + "Próximo passo")

- **Fatia 3b LANDED na branch `feat/datasets-storage` (3b.0–3b.7:
  `f6c6ff5`..`393163c`) + docs sincronizados (3b.8, working tree desta sessão, sem
  commit — o coordenador commiteia). Branch à frente de `main`; merge = decisão do
  usuário. Entregue: migration 0003 + `StoragePort`/`MockStorage`/`S3Storage` + 6 rotas
  (upload, images, detail, `/data`, boxes, caption) + sweep pós-commit + `source`
  derivado + `classes{id}`; spec 0.3.0; revisões 3b.3/3b.6 feitas. Dívida 3b QUITADA
  (ver "Dívidas"); sobraram: logging server-side (fatia nomeada), gate `sub` órfão,
  `cargo fmt`. A/B 3b.6 registrado no bullet do experimento (placar 3b.3 empate 2×2;
  swap = decisão do usuário).

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
- **Experimento A/B de revisor (3b)**: `@reviewer-max` (qwen3.8-max, `variant: high`,
  corpo idêntico ao titular) despacha no MESMO diff que o `@reviewer` nos marcos
  3b.3 e 3b.6; compara-se achados reais e falsos positivos; critério de swap ao fim
  da 3b. Nota operacional: agente novo só entra na lista de subagentes após reinício
  de sessão (dispatcher de reserva durante a 3b: `opencode run --auto --agent
  reviewer-max`). *Dia 1 (smoke em `86fb0eb`)*: ambos BLOQUEIA no mesmo defeito real
  (gate de segredos × `!.env.example` do gitignore — comprovado por matriz de 4 casos
  antes do fix); o titular ainda cruzou com a ADR-0003 (`.env.example` é entregável
  prometido da 3b.4)   — 1 ponto pro flash por enquanto. *Marco 3b.3 (difícil, teste real)*:
  ambos **BLOQUEIA** pelo MESMO crítico comprovado por sonda própria (livelock multipart pós-
  `LengthLimit` — axum embrulha corpo todo, multer nunca fuseja; os dois citaram a fonte e
  reproduziram), **zero falso positivo nos dois lados**. Titular achou a mais: duplicate falso
  por stem sem extensão (F4) e a janela de boot mock (F6, decisão do coordenador); sombra achou
  a mais: doc de `sanitize_filename` mentindo + branch morto (F7) e o staleness latente do
  `COALESCE(NEW,OLD)` em UPDATE de reparentização (registro: inofensivo até existir rota de
  UPDATE de `image_id`/`dataset_id`). Contagem de achados únicos: 2 titular × 2 sombra —
  **empate técnico no marco**. *Marco 3b.6*: titular CONDICIONAL com **1 falso positivo**
  (emenda vista só no trunk — o diff do marco não continha o fix) e 2 únicos (erros por-chave
  do `delete_prefix`, TTL não wired no compose); sombra PASSA com 2 únicos (órfão
  `infra_pgdata` no runner, upsert com `RETURNING`) e 0 falsos. Vereditos divergentes, listas
  convergentes. **Placar: 3b.3 empate 2×2; 3b.6 2×2 com vantagem da sombra em falsos
  positivos.** Critério de swap ao fim da 3b é **decisão do USUÁRIO — apresentar, não
  decidir**; hipótese "medium no fixer reduz escaladas" segue em teste.
- **Esforço de razonamento fixado por agente** (`variant:` na frontmatter, validado
  no provider): `high` em hephaestus/architect/reviewer (+ sombra max), `medium` em
  fixer e ui-designer, `low` nos implementadores e explore. Hipótese a medir na 3b:
  `fixer` com medium deve reduzir escaladas ao coordenador; se `low` em implementador
  degradar obediência à spec, subir para medium é uma linha. Vale a partir da
  próxima sessão (config de agente não retroage em sessão viva).

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

## Dívidas registradas que as próximas fatias precisam honrar

- **3b (upload/imagens) — QUITADA 2026-09-05** (`feat/datasets-storage`:
  `f6c6ff5` migration 0003, `656a474` porta/mock/AppState, `c012be5` upload+lista,
  `a21345b` fix livelock/413/filename, `507637e` S3+compose+runner, `94fc0db`
  detail+`/data`, `8451fa0` boxes+caption, `3b6b46e` fix TTL/sweep, `393163c` sweep+
  `source`+classes com `id`; docs sincronizados no 3b.8). Entregue: bucket S3/SeaweedFS
  como blob canônico (volume `datasets`/`DATASETS_DIR` do principal mortos; sweep de
  prefixo `datasets/<id>/` pós-commit best-effort), `DefaultBodyLimit` dedicado
  (total 200 MiB + 8 MiB + teto por arquivo 200 MiB, envelope — ADR-0002 T10),
  `images.object_key` + `sha256`/`media_type`, `videos` sem rota de escrita,
  `heph_refresh_dataset_counters` (nunca `+=`, status derivado), `source` derivado
  `s3://{bucket}/datasets/{id}/`, `classes` como `{id,name,idx,color}` (gap classId,
  3b.7). O que SOBROU para as fatias seguintes: export/import/package → **3e**
  (backup interim = console + `mc mirror`); bloco `--datasets` no
  `scripts/e2e-smoke.sh` (se ainda não coberto); UI `/datasets` (3c) e
  galeria/anotação (3d). Verificado: lista, `GET /:id` e `POST` preenchem `classes`
  com `{id,name,idx,color}` (`handlers.rs:80,237,277`).
- **Jobs (fatia 4)** — `jobs.dataset_id UUID NULL REFERENCES datasets(id)
  ON DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`) — ADR-0002 T4.
- **3d** — derivar `autoTracked` de `boxes.origin='autotracker'` (T7); hoje é
  constante `false`.
- **Hardening (sem fatia marcada)** — gate aceita `sub` órfão: cookie assinado com
  segredo antigo sobrevive a reset de `users` e passa a ler/deletar datasets (T8;
  mitigação = `SELECT EXISTS` no gate ou rotacionar segredo no reset). `cargo fmt
  -p api-principal` segue violando padrão (hunks pré-existentes da Fatia 2 +
  acréscimo da 3b); nenhum CI de fmt — decisão de quando formatar é do usuário.
- **Logging server-side (fatia nomeada, sem número)** — revisores 3b.3/3b.6: `Err(_)
  => internal()` engole detalhes (banco vira 500 mudo, sem log nenhum) e o `map_err`
  do SDK descarta `code()`; sweep do DELETE usa `eprintln` como mínimo honesto até
  a fatia chegar. Escopo: log server-side (nunca no response) antes/depois da fatia
  de jobs.

## Plano em andamento — PRÓXIMO PASSO EXATO: merge da `feat/datasets-storage` → 3c

**3b.0–3b.8 FEITOS 2026-09-05 (ver "Estado atual").** A sequência:

1. ~~`spike/storage-seaweedfs`~~ ✅ **CONCLUÍDO 2026-09-05** (ramo `spike/storage-seaweedfs`,
   commit `09a517d`, matriz em `spike/STORAGE-SPIKE.md`; **fundido pelo usuário em `main` (`52da6f9`)** — harnesses e matriz vivem no tronco).
2. ~~**3b.1..3b.7**~~ ✅ **CONCLUÍDOS** em `feat/datasets-storage` (`f6c6ff5`..`393163c`);
   `@reviewer` ao fim de 3b.3 e 3b.6 (ver A/B no "Estado atual").
3. ~~**3b.8**~~ ✅ **CONCLUÍDO nesta sessão** (`@docs-sync`: deltas da ADR-0003 aplicados em
   `backend.md`/`frontend.md` + banner na ADR-0002 + ADR-0003 marcada IMPLEMENTADA).
4. Depois (decisão do usuário): **merge da branch** → **3c UI `/datasets`** (lista) →
   **3d galeria+annotate** → **3e export/import** → **4 jobs/package/materialização**
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
- [ ] Próximo passo = **merge da `feat/datasets-storage`** (decisão do usuário) → 3c.
- [ ] Pendências que continuam valendo, sem fatia marcada: CLI
      `studio reset-password` (ADR-0001 T4), `cargo fmt -p api-principal` segue,
      logging server-side (fatia nomeada — revisores 3b.3/3b.6), gate `sub` órfão
      (ADR-0002 T8).
      ~~`lefthook install`~~ ✅ quitado em `chore/agent-team` (gate ativo: hooks
      instalados + commitlint real + deny de commit nos subagentes).
