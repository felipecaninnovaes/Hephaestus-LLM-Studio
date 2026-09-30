# Infraestrutura — Pendências Ativas

> **Data de Consolidação:** 2026-09-26  
> **Status:** Ativo  
> **Origem:** Fusão de `tasks/specs/infra-auditoria.md` (20 itens) + `tasks/specs/infra-autonomia.md` (20 itens). Este documento contém **apenas os itens confirmados como genuinamente não implementados** conforme auditoria de código 2026-09-26.  
> **Itens Implementados e Removidos:** INFRA-14 (heartbeat via env), INFRA-19 (Next.js standalone), 4.1 (IPs dinâmicos em compose.gpu.yaml), 5.2 (CI Python engines), 2.1 (USER não-root em api-principal).

---

## P0 — Crítico

### [ ] INFRA-03: Segregação e injeção dinâmica de credenciais S3 (SeaweedFS)
- **Arquivo:** `infra/seaweedfs-s3.json:1-32` (versionado com senhas fracas)
- **Problema:** Credenciais `heph-local-dev` e `heph-orch-local-dev` estão em texto claro no repositório. Em produção, montar esse arquivo expõe o S3 a qualquer pessoa com acesso ao código.
- **Ação Recomendada:**
  1. Criar `infra/seaweedfs-s3.json.example` com placeholders
  2. Gerar dinamicamente o JSON via script ou Docker Secrets
  3. `compose.prod.yaml` monta arquivo gerado fora do versionamento
- **Critério de Aceite:** Repositório não versiona credenciais utilizáveis em produção.

---

## P1 — Alto

### [ ] 2.2: Executar orchestrator com usuário não-privilegiado (`USER`)
- **Arquivo:** `services/orchestrator/Dockerfile:34` (CMD sem USER anterior)
- **Problema:** Orchestrator roda como `root` (UID 0), e monta `/var/run/docker.sock`. Um escape de container concede privilégios totais no host.
- **Ação Recomendada:**
  1. Criar usuário e grupo `studio:1000` no Dockerfile
  2. Ajustar permissões do diretório `/app`
  3. Adicionar `USER studio` antes de `CMD`
- **Critério de Aceite:** `docker inspect hephaestus/orchestrator` mostra `Config.User == "1000:1000"`.

### [ ] 2.4: Modernizar cache de builds Cargo com `cargo-chef`
- **Arquivos:** `services/*/Dockerfile` (todos usam dummy `fn main() {}`), exemplos em `services/orchestrator/Dockerfile:8-20`, `services/manager/Dockerfile:8-20`
- **Problema:** Uso artesanal de dummy `fn main() {}` para cache de dependências. Invalida cache em mudanças triviais.
- **Ação Recomendada:**
  1. Adicionar stage `planner` com `cargo-chef plan`
  2. Adicionar stage `cook` com `cargo-chef cook --release`
  3. Aplicar a todos os Dockerfiles Rust
- **Critério de Aceite:** Builds Cargo reutilizam cache de dependências mesmo com mudanças de src.

### [ ] 3.2: Parametrizar credenciais do SeaweedFS via variáveis de ambiente
- **Arquivos:** `infra/seaweedfs-s3.json` (estático), `infra/compose.yaml` (s3-init sem template)
- **Problema:** Credenciais S3 e identidades estão fixas em arquivo JSON sem interpolação de secrets.
- **Ação Recomendada:**
  1. `s3-init` gera dinamicamente `/etc/seaweedfs/s3.json` a partir de `S3_ACCESS_KEY`, `S3_SECRET_KEY`, etc.
  2. Mount do arquivo gerado no container `seaweedfs`
- **Critério de Aceite:** Credenciais de produção vêm exclusivamente de variáveis de ambiente, nunca versionadas.

### [ ] 3.4: Implementar exportador de métricas (Prometheus) nos serviços Axum
- **Arquivos:** `services/api-principal/src/main.rs`, `services/manager/src/main.rs`, `services/orchestrator/src/main.rs` (nenhum implementa `/metrics`)
- **Problema:** Ausência de endpoint `/metrics` para telemetria de conexões de banco, requisições HTTP, latência e fila de jobs.
- **Ação Recomendada:**
  1. Integrar `metrics-exporter-prometheus` (ou similar) em cada serviço
  2. Expor `/metrics` em porta separada ou rota interna
  3. Documentar métricas relevantes (conexões ativas, latência p99, fila de jobs)
- **Critério de Aceite:** `curl http://localhost:8080/metrics` retorna output em formato Prometheus; padrão `# HELP` e `# TYPE`.

---

## P2 — Médio

### [ ] 2.5: Adicionar diretiva `HEALTHCHECK` nativa em todos os Dockerfiles
- **Arquivos:** `services/api-principal/Dockerfile`, `services/manager/Dockerfile`, `services/orchestrator/Dockerfile` (nenhum possui `HEALTHCHECK`)
- **Problema:** Falta de probes embutidas nas imagens para orquestradores que não utilizam compose.yaml (ex.: Kubernetes).
- **Ação Recomendada:**
  1. Adicionar instrução `HEALTHCHECK` ao final de cada Dockerfile
  2. Consumir endpoints `/health` ou `/ready` já existentes
  3. Configurar timeouts e retries conservadores (ex.: 10s interval, 5s timeout, 3 retries)
- **Critério de Aceite:** `docker run ... --health-cmd` passa; `docker ps` mostra `(healthy)` após stabilize.

### [ ] 3.3: Definir regras de Lifecycle e expiração de artefatos temporários no S3
- **Arquivo:** `infra/seaweedfs-s3.json` (sem regras de lifecycle) ou `services/manager/src/` (sem lógica de purga)
- **Problema:** Amostras intermediárias e uploads de treinos descartados acumulam indefinidamente no volume `seaweed_data`.
- **Ação Recomendada:**
  1. Implementar regras de lifecycle via SeaweedFS (se suportado) ou via job do `manager`
  2. Purga automática de artefatos não consolidados após 30 dias
  3. Logs de deletions para auditoria
- **Critério de Aceite:** Artefatos não consolidados > 30 dias são removidos do S3 automaticamente; logs via Prometheus ou structured logging.

### [ ] 5.3: Otimizar caching e dependências do runner no CI
- **Arquivo:** `.gitea/workflows/ci.yml:12` (documenta ausência de cache)
- **Problema:** CI reinstala `git`, `rustfmt`, `cargo` deps e npm deps a cada execução.
- **Ação Recomendada:**
  1. Usar imagem base customizada para CI (com ferramentas pré-instaladas)
  2. OU habilitar cache de cargo via `.cargo/registry` e npm via `.npm-cache`
  3. Considerar container registry privado com base pré-compilada
- **Critério de Aceite:** Tempo de CI reduzido em ≥30%; segunda execução sem mudança roda <50% do tempo da primeira.



### [ ] 5.5: Job timeout por job (watchdog adicional além de nó offline)
- **Arquivo:** `services/manager/src/`, `tasks/specs/backend-autonomia.md:5.3`
- **Problema:** Orquestrador finaliza mas report falha; job fica `running` eternamente pois nó ainda está online.
- **Ação Recomendada:**
  1. Adicionar timeout por job: status `running` sem telemetria por 2x `heartbeat_interval`
  2. Watchdog transiciona para `failed` com `queue_reason='job_timeout'`
  3. Registrar timeout em telemetria/logs
- **Critério de Aceite:** Job em `running` por >120s sem telemetria é marcado como falha; teste de integração prova.


---

## Referência de Itens Implementados e Fechados

Os itens a seguir foram confirmados como **implementados e quitados** na auditoria de 2026-09-26:

| ID Anterior | Descrição | Confirmação |
| :--- | :--- | :--- |
| **INFRA-14** | Heartbeat/timeout via variáveis de ambiente | `services/orchestrator/src/main.rs:272` — `HEARTBEAT_INTERVAL_SECS` env var |
| **INFRA-19** | Next.js output `standalone` para reduzir tamanho | `apps/web/next.config.ts:8` — `output: "standalone"` |
| **4.1** (autonomia) | Desacoplar IPs estáticos em compose.gpu.yaml | `infra/compose.gpu.yaml:32-39` — `${MANAGER_URL:?}`, `${S3_ORCH_ENDPOINT_URL:?}`, etc. com fail-fast |
| **5.2** (autonomia) | CI Python engines | `.gitea/workflows/ci.yml:122-146` — pytest em trainer-yolo, trainer-difusao, trainer-clip |
| **2.1** (autonomia / api-principal) | USER não-root (api-principal) | `services/api-principal/Dockerfile:30` — `USER studio` |
| **2.1** (autonomia / manager) | USER não-root (manager) | `services/manager/Dockerfile:25` — `USER studio` |
| **1.3** (autonomia) | Segregar redes no Docker Compose | Implementado via `feat/infra-redes-segmentadas` — redes segmentadas (frontend_net, backend_net, engine_net) |

---

## Como Usar Este Documento

1. **Triagem:** Cada item possui checkbox `[ ]` para rastreamento de progresso.
2. **Prioridades:** P0 (crítico) deve ser abordado em wave imediata; P1/P2 em waves subsequentes.
3. **Acceptance:** Critério de aceite explícito; nenhuma tarefa considera-se concluída sem evidência comprovada.
4. **Tracking:** Este arquivo é único; não criar specs adicionais — todas as pendências aqui.
