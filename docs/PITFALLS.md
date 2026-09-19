# PITFALLS — Registro de Armadilhas (memória de longo prazo)

Índice compacto de falhas que o projeto **já pagou em tempo ou dados**:
sintoma → causa-raiz → regra. Ler a seção do pilar ANTES de tocar naquele
código. Rotina de manutenção no § Critério. Manter <100 linhas (funil L0→L3;
este arquivo é a única memória lida inteira).

## Critério de gravação (sobe para cá se qualquer um for verdadeiro)

1. **Custo real**: >30 min de depuração, bug achado em E2E/smoke, ou finding
   [MAIOR] de `@reviewer`.
2. **Recorrência**: 2ª falha do mesmo erro (Regra das Duas Correções) — ao
   fechar, a lição é PROMOVIDA de `tasks/active.md` para cá.
3. **Silencioso/destrutivo**: falha sem log, dado órfão/perdido, status
   errado no wire, superfície de segurança.

NÃO gravar: transient resolvido na hora sem chance de retorno; comportamento
óbvio de framework; nada que já seja regra inegociável do `AGENTS.md`.
Roteamento: decisão com ressalva → fica no `Gotcha:` da ADR; se a ressalva
ultrapassa a decisão (vale para qualquer sessão futura no pilar), duplica-se
aqui com a fonte. Trabalho futuro → `tasks/backlog.md`.

## Formato

`- **Sintoma observável** → causa-raiz → regra permanente. (fonte)`

## Serviços — Rust/Axum

- **Rota protegida sem cookie cai em 404 cru em vez de 401** → `.layer()` do axum 0.7 NÃO cobre caminhos não roteados pelo matchit → gate com `.route_layer()` + isenção por prefixo (`/api/auth/*`, `/health`). (ADR-0001)
- **Referência inexistente a catálogo responde 503** → `submit_yolo_job` mapeia todo `Err(_)` do manager como 503, mascarando NotFound → handlers novos distinguem `NotFound → 404` / `InvalidRequest → 400`; corrigir o legado exige escopo separado. (ADR-0013, ADR-0014 R6)
- **Wire camelCase vs JSONB snake_case** → `params` interno (ex.: `package_ref`) é snake_case; o contrato público é camelCase → nunca vazar shape interno no wire; codegen espelha `openapi.yaml`, não o Postgres. (ADR-0007, `packages/contracts`)

## Storage — S3/SeaweedFS

- **Presigned URL quebrada no browser** → o header `Host` entra na assinatura SigV4 → presigned SEMPRE assinado com `S3_PUBLIC_ENDPOINT_URL` (host alcançável pelo browser), nunca o endpoint interno dos containers. (ADR-0003, revisitado ADR-0010)
- **`List` no SeaweedFS é bucket-level** → IAM policy com prefixo (`List:.../packages/*`) pode NÃO autorizar `ListObjectsV2` com `prefix=` → validar permissão de List no Spike antes de desenhar varredura por prefixo. (ADR-0007 R1)
- **`copy_object` 400** → `copy_source` exige key URL-encoded com barras preservadas (`{bucket}/{key}`) → encode na camada StoragePort, não no chamador. (ADR-0005)
- **Objects órfãos silenciosos no bucket** → sweep do `DELETE /:id` cobre `datasets/{id}/` mas NÃO `packages/<version_id>/`; `size_bytes` exclui soft-deleted até o purge → reconciliação S3×Postgres periódica é obrigatória antes de confiar em qualquer conta de armazenamento. (ADR-0007 R6, backlog §3)

## Orquestração — manager/orchestrator

- **Nó registra heartbeat órfão e vira degraded/offline sem erro claro** → `ORCH_ADVERTISE_URL` errado (URL que o manager não alcança) → warn no log do manager; conferir advertise URL em todo pareamento novo. (ADR-0011 D1)
- **Job cancelado aparece como `failed`** → abort durante `preparing`/`dispatched` não mapeava status → qualquer transição nova precisa do caminho abort-testado; hotfix `cancelled` pós-abort no `report_job` do manager. (ADR-0007; RD-021 Wave 2; hotfix 4bfb450)
- **Watchdog mata build legítimo >60 min** → âncora era `created_at` → watchdog de `preparing` ancora no `updated_at` (heartbeat do worker). (backlog §2)
- **Partição de rede permite dupla execução** → recovery por heartbeat não distingue nó morto de nó isolado → risco ACEITO: artefatos são last-write-wins por `job_id` com md5 determinístico por (params, seed); mudar isso exige fencing, não retry. (ADR-0011 R3)
- **`--gpus "device=N"` pega a GPU errada após reboot** → N é índice nvidia-smi do HOST e a ordem pode mudar → smoke valida `nvidia-smi -L` antes da sessão GPU. (ADR-0010 D9)

## Engines — Python/ultralytics

- **Parse de métricas quebra sem mudança de código** → ultralytics renomeia colunas do `results.csv` entre minor versions → pin `8.3.x` no Dockerfile + teste de parse fixam o contrato. (ADR-0010)
- **VRAM do card parece "inflada"** → `nvidia-smi memory.used` é GLOBAL por GPU (inclui host/outros containers) → é o número correto para o card do nó; não subtrair processos. (ADR-0010)
- **`jobs.engine='world'` mente sobre o executor** → `engine='world'` vive SÓ na tabela `models`; o job roda `engine='autotracker'` (imagem trainer-yolo) → dispatch/rotaamento leem `jobs.engine`, catálogo lê `models.engine`. (ADR-0014)
- **`uv run pytest` falha na raiz do repo** → não há projeto uv raiz; uv é POR engine (`cd engines/<engine>`). (AGENTS §2)

## Frontend — apps/web

- **Conteúdo inalcançável no mobile (provado por medida, não opinião)** → scroll aninhado: `overflow-y-auto` em pai E filho de `flex-col` → só o `main` rola (Anti-Scroll-Trap, DESIGN.md); auditado com sh/ch medidos. (histórico sessão 14; DESIGN.md)
- **Verde legado da v1 reaparece** → classes `emerald-*` ativam a paleta v1 no Tailwind v4 → Brand-Only; verde semântico só como literal `#34d399`. (DESIGN.md §Don'ts)
- **Geração trava atrás de treino longo** → predict e train compartilham FIFO única do manager → comportamento conhecido do v1; prioridade de fila é dívida, não bug. (ADR-0013 R3)

## Contexto & Processo

- **Sessão queimou ~45M tokens de input** → histórico reenviado por chamada + leituras de arquivos inteiros + ANSI de hooks → funil L0→L3 obrigatório; `docs/archive/` é NUNCA lido (lições vivas estão AQUI); "Documentar e Limpar" perto de 40%. (histórico 2026-09-16; Regra 6)
