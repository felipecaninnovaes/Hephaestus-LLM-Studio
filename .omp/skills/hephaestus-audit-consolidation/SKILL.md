---
name: hephaestus-audit-consolidation
description: Consolidação transversal das 4 auditorias (web, backend, engines, infra) e auditoria dos contratos e políticas (packages/contracts, packages/policies). Cruza contratos OpenAPI, protocolo manager↔orchestrator, contrato de execução de engines e segredos para gerar um roadmap único com ordem de dependência em tasks/consolidacao-auditoria-roadmap.md.
---

# Objetivo
Fazer a CONSOLIDAÇÃO TRANSVERSAL E AUDITORIA DE CONTRATOS do Hephaestus LLM Studio.
Lê os 4 relatórios de auditoria dos pilares (`tasks/web-modularizacao-auditoria.md`, `tasks/backend-modularizacao-auditoria.md`, `tasks/engines-modularizacao-auditoria.md`, `tasks/infra-auditoria.md`), audita o ponto de encontro de todas as camadas (`packages/contracts/` e `packages/policies/`), cruza as 4 fronteiras intercamadas e gera um Roadmap Único Unificado com grafo de dependência rigoroso, dividido em Ondas (Waves).
"Independência e autonomia" significa: a evolução de qualquer pilar é guiada por contratos explícitos e auditados, e o plano de ação unificado impede que uma camada seja refatorada antes de suas dependências canônicas estarem prontas.

# Regras
- Auditoria SOMENTE LEITURA. Não edite código, configurações, schemas de banco ou containers.
- O único arquivo gerado é `tasks/consolidacao-auditoria-roadmap.md`.
- Trate todas as documentações em `docs/` como hipóteses sujeitas à validação frente ao código real.
- Toda afirmação de convergência, divergência ou risco intercamada deve citar evidências exatas de cada lado da fronteira (`arquivo:linha`).
- Se houver conflito entre código e documentação, a regra inegociável é: o contrato canônico (`packages/contracts/openapi.yaml` e `packages/policies/`) é a meta, e o código real é o ponto de partida do roadmap.

# Fase 0 — Verificação de Insumos e Pré-requisitos
1. Verificar a existência dos relatórios dos 4 pilares em `tasks/`:
   - `tasks/web-modularizacao-auditoria.md` (produzido via `skill://hephaestus-audit-web`)
   - `tasks/backend-modularizacao-auditoria.md` (produzido via `skill://hephaestus-audit-backend`)
   - `tasks/engines-modularizacao-auditoria.md` (produzido via `skill://hephaestus-audit-engines`)
   - `tasks/infra-auditoria.md` (produzido via `skill://hephaestus-audit-infra`)
   *Nota de execução:* Caso alguma auditoria individual ainda não tenha sido concluída, a consolidação executa uma amostragem focalizada daquele pilar nos pontos de contato com as outras camadas e sinaliza a pendência no roadmap.
2. Carregar o contexto macro do repositório:
   - `AGENTS.md` (L0) e `docs/REPO_MAP.md` (L1)
   - `docs/architecture/overview.md` e `docs/architecture/network-and-vram.md`

# Fase 1 — Auditoria Direta dos Pacotes Compartilhados (`packages/`)
Auditar diretamente os arquivos em `packages/` que servem como ponto de encontro do monorepo e até hoje não estavam cobertos por nenhuma auditoria isolada:
1. `packages/contracts/openapi.yaml`:
   - Sintaxe, validade do schema OpenAPI 3.0/3.1 e integridade referencial (`$ref`).
   - Convenção de nomenclatura: verificar se todo o wire está rigorosamente em `camelCase`.
   - Completude dos endpoints: rotas documentadas vs rotas reais expostas em `services/api-principal/src/server/` e consumidas em `apps/web/src/`.
   - Modelos de dados e DTOs: schemas declarados vs structs Rust (`serde(rename_all = "camelCase")`) vs tipos TypeScript (`apps/web/src/types/studio.ts`).
   - Endpoints não documentados (endpoints "fantasmas" no backend ou frontend chamando rotas inexistentes).
2. `packages/policies/vram-table.yaml`:
   - Estrutura de entradas (`entries`): engine, model, mode (`train` vs `generate`/`infer`), `vram_min_gb`.
   - Defaults e políticas: `headroom_gb`, `measure_margin`, `default_train_gb`.
   - Consistência com o hardware real documentado (ex: GTX 1660S 6GB vs RTX 3060 12GB vs GPUs de nós remotos) e com os handlers do `manager` (`services/manager/src/scheduler/`) e `api-principal` (`handlers.rs` / `vram.rs`).
   - Cobertura de modelos: modelos listados na tabela vs modelos implementados em `engines/trainer-difusao/` (SD1.5, SDXL, Flux, Flux2-Klein) e `engines/trainer-yolo/` (yolo11n, yolo11m, yolo11s, etc.).
3. `packages/policies/engines.yaml`:
   - Imagens oficiais de engines (`hephaestus/trainer-*:local` e `:gpu`).
   - Toolchains canônicas registradas (`cuda`, `torch`, `ultralytics`, datas de validação).
   - Alinhamento com os Dockerfiles em `engines/` e com as imagens usadas pelo `orchestrator` e no `infra/compose.gpu.yaml`.

# Fase 2 — Cruzamento das 4 Fronteiras Transversais
Cruzar os dados dos relatórios e do código para cada uma das 4 pontes estruturais:

## Fronteira 1: Contrato OpenAPI e Alinhamento Web ↔ BFF (`api-principal`)
- **Tipos e Drift:** `apps/web/src/types/studio.ts` é manual ou gerado? Onde há campos inventados no frontend que o backend não retorna, ou campos obrigatórios no backend que o frontend omite?
- **Tratamento de Erros:** O formato de erro retornado pelo `api-principal` (ex: status codes, body JSON `{ error, message, code }`) é consumido uniformemente pelos hooks e fetchers do frontend?
- **Streaming e SSE:** Como o endpoint `/api/jobs/{id}/telemetry` ou `/api/jobs/{id}/events` trafega do backend para a UI? O Next.js proxy/rewrite em dev e o Caddy em prod tratam SSE com flushing imediato (sem buffer)?
- **Uploads Chunked e Hash:** O protocolo de upload em partes (partes até 96 MiB, cálculo de MD5, streaming direto para TempDir) está alinhado entre o frontend (`apps/web/src/lib/upload/` ou hooks) e o backend (`services/api-principal/src/models/upload.rs`)?

## Fronteira 2: Protocolo Interno Manager ↔ Orchestrator
- **Assimetria de DTOs:** Como o manager despacha jobs (`JobDispatchPayload` / `POST /jobs/dispatch`) e como o orchestrator recebe? Há duplicação de tipos Rust ou divergência de campos opcionais?
- **Ciclo de Vida do Job e Heartbeat:** O orchestrator reporta progresso via `POST /jobs/report`. O manager atualiza a máquina de estados (`queued` -> `preparing` -> `running` -> `done`/`failed`/`cancelled`). O heartbeat de 5s do orchestrator casa com a janela de 10s de nó `stale` do manager?
- **Cancelamento:** Como o cancelamento pedido pelo usuário na Web passa: `Web -> api-principal -> manager -> orchestrator -> container engine`? O sinal ou flag chega limpo até o processo Python?
- **Segurança de Pareamento e Tokens:** O `MANAGER_TOKEN` e o mecanismo de HMAC de pareamento de nós (`docs/infra/gpu-nodes.md`) são validados de forma consistente entre manager e orchestrator?
- **Estratégia de Deploy Coordenado:** O que acontece se o manager for atualizado e nós remotos ainda estiverem na versão anterior do orchestrator? Definir política de versionamento de protocolo interno (`/v1/...` ou compatibilidade aditiva).

## Fronteira 3: Contrato de Execução e Telemetria Orchestrator ↔ Engines
- **Parâmetros e Invocação:** Quais flags, volumes e env vars o orchestrator passa no `docker run` (`services/orchestrator/src/app/` e `src/adapters/docker.rs`) vs o que `train.py`, `autolabel.py` e `serve.py` esperam?
- **Paths de Dados:** Mapeamento `/data/inputs`, `/data/weights`, `/data/outputs`, `/data/telemetry`. Existe risco de path traversal ou permissões incompatíveis (UID/GID non-root)?
- **Telemetria Unificada:** O `telemetry.jsonl` gerado pelo `TelemetryEmitter` (`engine-kit`) com progresso 0.0-1.0 é lido pelo orchestrator sem truncamento e repassado íntegro ao manager e à UI? O arquivo legado `metrics.jsonl` ainda é necessário?
- **Daemons Persistentes vs Jobs One-Shot:** Como o daemon de difusão (`DIFFUSION_DAEMON_IDLE_TTL_S`) e o CLIP (`trainer-clip`) mantêm estado aquecido, inferência atômica (`_busy`) e shutdown gracioso?
- **Mock Determinístico e Assinatura:** O orchestrator confia na assinatura `HEPHMOCK` para pipelines de teste rápido sem GPU? A paridade entre `mock_vector` (Python) e `MockEmbedder` (Rust) é garantida por contrato?

## Fronteira 4: Topologia de Rede, Segredos e Superfície de Ataque
- **Ciclo de Vida de Segredos:** Inventário ponta a ponta: `STUDIO_PASSWORD`, `STUDIO_MASTER_KEY`, `AUTH_SECRET`, `MANAGER_TOKEN`, `POSTGRES_PASSWORD`, credenciais SeaweedFS S3.
- **Fail-Fast em Produção:** Garantir que o compose de prod (`compose.prod.yaml`) impeça a subida se qualquer uma das variáveis críticas estiver ausente ou com valor padrão de exemplo.
- **Isolamento de Rede:** Garantir que nenhuma engine exponha portas no host (`ports:` proibido nas engines).
- **Exposição de Nós e LAN:** Se o nó GPU precisa acessar `manager:8081` e `seaweedfs:8333` via LAN, como isso é autenticado e protegido contra acessos não autorizados?
- **Cookie e Sessão:** O cookie de sessão `heph_session` usa flags `HttpOnly`, `SameSite=Lax/Strict` e `Secure` (com suporte a `X-Forwarded-Proto` atrás do Caddy)?

# Fase 3 — Grafo de Dependências e Matriz de Impacto
1. Montar a Matriz de Dependência Cruzada entre os achados das 4 auditorias e de packages:
   - Identificar dependências circulares ou bloqueios mútuos.
   - Definir qual camada DEVE mudar primeiro para destravar as seguintes.
2. Identificar "Breaking Changes" e Riscos de Regressão:
   - Mudanças que quebram contrato HTTP público (Web afetada).
   - Mudanças que quebram protocolo interno (nós remotos afetados).
   - Mudanças que exigem migração de banco de dados ou recriação de volumes S3.
   - Mudanças que exigem rebuild de imagens Docker (`trainer-*:local` e `:gpu`).

# Fase 4 — Construção do Roadmap Único Unificado em Ondas Sequenciais (Waves)
Agrupar todos os achados e refatorações em ondas cronológicas estritas, onde nenhuma tarefa de uma onda depende de algo de uma onda posterior:

### Onda 0 — Fundação, Contratos Canônicos e Guardrails (Sem quebra de runtime)
- Correção e trava dos contratos em `packages/contracts/openapi.yaml`.
- Alinhamento das tabelas em `packages/policies/` (`vram-table.yaml`, `engines.yaml`).
- Fail-fast de segredos em produção e guardrails contra perda de dados no compose (`down -v`).
- Fixação de ferramentas de governança e lints cruzados.

### Onda 1 — Pacotes e Bibliotecas Compartilhadas (Fundação de Código)
- Backend: Criação/extração dos crates compartilhados de contratos/DTOs, protocolo interno, erros e telemetria.
- Engines: Consolidação do `engine-kit` (settings centralizadas, telemetria atômica, interfaces de mock/real).
- Web: Tokens de design `@theme` consolidados e primitivos `components/ui/` desvinculados de regras de negócio.

### Onda 2 — Camada Interna de Execução e Orquestração (Nós e Infraestrutura)
- Conclusão da modularização do `orchestrator` e runners Docker.
- Ajustes de estabilidade no `manager` (scheduler, heartbeat, reconciliação de órfãos).
- Refatoração dos trainers de Difusão, YOLO e CLIP para uso estrito do `BaseModelTrainer` e `engine-kit`.
- Configuração de rede, storage S3 ACLs e bindings seguros de nó GPU.

### Onda 3 — Borda e Serviços de Aplicação (BFF e Domínio)
- `api-principal`: Handlers limpos, sem SQL inline, desacoplados via traits, respeitando 100% o OpenAPI.
- Migrações de banco e isolamento de acesso Postgres entre `api-principal` e `manager`.
- SSE e streaming de uploads robustos e auditados.

### Onda 4 — Frontend e Interface do Usuário (Apps / Web)
- Quebra dos arquivos gigantes em `components/studio/` por feature.
- Desacoplamento de fetch e regra de negócio das páginas e telas.
- Alinhamento de `types/studio.ts` com o contrato OpenAPI gerado/validado.
- Aplicação das convenções Impeccable (acessibilidade, responsividade, estados loading/erro/vazio).

### Onda 5 — Observabilidade, E2E e Limpeza Final
- Testes de integração E2E herméticos cobrindo a cadeia completa em mock.
- Rotação de logs e métricas consolidadas.
- Remoção cirúrgica de código morto, DTOs obsoletos e shims temporários.

# Entregável
Criar `tasks/consolidacao-auditoria-roadmap.md`, seguindo as convenções de tasks do AGENTS.md, contendo:
1. Resumo Executivo da Arquitetura Holística do Hephaestus (estado atual vs arquitetura alvo).
2. Auditoria Direta de `packages/contracts` e `packages/policies` (achados, drifts e propostas).
3. Matriz das 4 Fronteiras Transversais (Web↔BFF, Manager↔Orchestrator, Orchestrator↔Engines, Infra/Segurança).
4. Grafo e Matriz de Dependência Intercamadas (tabela de impacto cruzado).
5. Roadmap Único Unificado em Ondas (Wave 0 a Wave 5), onde cada item possui:
   - ID único (ex: `RD-01`, `RD-02`).
   - Título e Camadas Afetadas (Apps, Services, Engines, Infra, Contracts).
   - Origem (ID correspondente na auditoria de origem).
   - Pré-requisitos / Dependências duras.
   - Proposta de Mudança e Critério de Aceite Objetivo.
   - Risco e Estratégia de Rollout/Compatibilidade.
   - Esforço (P/M/G) e Subagente Sugerido (`@architect`, `@rust-dev`, `@frontend-dev`, `@python-engines`, `@infra-dev`).
6. Guia de Implementação e Guardrails para o Coordenador (`AGENTS.md`).

No chat, responda apresentando a visão geral do roadmap consolidado, os conflitos intercamadas mais críticos detectados e os próximos passos.
