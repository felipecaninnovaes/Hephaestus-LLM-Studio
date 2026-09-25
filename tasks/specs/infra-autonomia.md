# Levantamento de Infraestrutura — Rumo à Autonomia e Independência

Documento de mapeamento e checklist de execução de todas as melhorias de infraestrutura do **Hephaestus LLM Studio**, visando estabilidade, segurança, escalabilidade e prontidão para operação autônoma e self-hosting.

Data do levantamento: 2026-09-18.

---

## 1. Topologia de Rede, Portas e Isolamento

- [ ] **1.1 Fechar ou isolar porta do Postgres no Host (`5432:5432`)**
  - **Severidade:** Crítico
  - **Arquivo:** `infra/compose.yaml:12`
  - **Problema:** A porta `5432` está vinculada a `0.0.0.0`, acessível na rede local com a senha padrão (`studio`).
  - **Ação:** Remover o mapeamento de portas para o host no modo autônomo (comunicação entre containers via rede interna `db:5432`) ou utilizar bind estrito no loopback (`${DB_PUBLISH:-127.0.0.1}:5432:5432`).

- [ ] **1.2 Injetar `--network` no spawn de containers pelo Orchestrator**
  - **Severidade:** Crítico
  - **Arquivos:** `services/orchestrator/src/lib.rs:989-1030` (`build_docker_run_args`), `services/orchestrator/src/daemon.rs:119-128` (`build_daemon_args`)
  - **Problema:** O comando one-shot `docker run` não passa `--network`, jogando containers na bridge padrão do Docker e quebrando resolução DNS interna de containers. O daemon de difusão faz fallback para `--network host`, violando a regra inegociável de isolamento de rede das engines.
  - **Ação:** Passar explicitamente `--network ${ENGINE_NETWORK:-infra_default}` no executor Docker e eliminar o fallback de rede host.

- [x] **1.3 Segregar redes no Docker Compose (Fim da Rede Flat)**
  - **Severidade:** Alto
  - **Arquivo:** `infra/compose.yaml`, `infra/compose.prod.yaml`
  - **Status:** Quitado (fatia `feat/infra-redes-segmentadas`).
  - **Implementado:** Redes segmentadas com isolamento estrito:
    - `frontend_net`: `ingress` (prod), `web` e `principal`. `web` sem acesso a `db` ou `seaweedfs`.
    - `backend_net`: `principal`, `manager`, `db`, `seaweedfs`, `s3-init`, `embedder` e `orchestrator-local`.
    - `engine_net`: `orchestrator-local`, `seaweedfs` (ponte segura para artefatos) e containers efêmeros de treino/daemon (`ENGINE_NETWORK=${COMPOSE_PROJECT_NAME:-infra}_engine_net`).

- [ ] **1.4 Implementar Ingress / Reverse Proxy Unificado (Caddy / Traefik / Nginx)**
  - **Severidade:** Alto
  - **Arquivo:** `infra/compose.yaml`
  - **Problema:** Atualmente 6 portas diferentes são expostas no host (`:3000`, `:8080`, `:8081`, `:8082`, `:8333`, `:9333`), sem suporte unificado a TLS ou domínio único.
  - **Ação:** Criar serviço de Ingress servindo portas padrão `:80`/`:443`, com terminação TLS automática e roteamento de rotas (`/` -> web, `/api/` -> principal, `/s3/` -> seaweedfs), fechando o acesso externo direto aos serviços internos.

---

## 2. Dockerfiles, Build Caching e Segurança de Containers

- [ ] **2.1 Corrigir `.dockerignore` do monorepo**
  - **Severidade:** Crítico
  - **Arquivo:** `.dockerignore`
  - **Problema:** Apenas ignora `apps/web/.next` e `apps/web/node_modules`. Envia gigabytes de `target/`, `.git/`, `.venv/`, caches e pesos pesados para o daemon do Docker a cada build.
  - **Ação:** Adicionar ao `.dockerignore`:
    - `target/`
    - `.git/`
    - `node_modules/` (raiz e subpastas)
    - `.venv/` e `**/__pycache__/`
    - `*.pt`, `*.bin`, `*.safetensors`
    - `datasets/`, `outputs/`, `models/`

- [ ] **2.2 Executar containers com usuário não-privilegiado (`USER`)**
  - **Severidade:** Alto
  - **Arquivos:** `apps/web/Dockerfile`, `services/*/Dockerfile`, `engines/*/Dockerfile*`
  - **Problema:** Todos os containers rodam como `root` (UID 0), incluindo serviços com montagem do Docker socket (`orchestrator`).
  - **Ação:** Definir usuário sem privilégios (ex.: `USER studio:studio` ou `USER node`) e ajustar permissões dos diretórios de trabalho `/app` e `/data`.

- [ ] **2.3 Padronizar base Debian de runtime para serviços Rust**
  - **Severidade:** Médio
  - **Arquivos:** `services/manager/Dockerfile:21` vs `services/api-principal/Dockerfile:26`
  - **Problema:** `manager` usa `debian:bookworm-slim` (GLIBC 2.36) enquanto o builder usa trixie (GLIBC 2.41), criando incompatibilidade binária com crates C (`aws-lc-sys`).
  - **Ação:** Padronizar `services/manager/Dockerfile` para `debian:trixie-slim`.

- [ ] **2.4 Modernizar cache de builds Cargo com `cargo-chef`**
  - **Severidade:** Médio
  - **Arquivos:** `services/*/Dockerfile`
  - **Problema:** Uso de dummy `fn main() {}` artesanal para cache de dependências.
  - **Ação:** Adicionar multi-stage com `cargo-chef` (fases `planner` e `cook`) para acelerar builds e evitar invalidações falsas de cache.

- [ ] **2.5 Adicionar diretiva `HEALTHCHECK` nativa em todos os Dockerfiles**
  - **Severidade:** Médio
  - **Arquivos:** `services/api-principal/Dockerfile`, `services/manager/Dockerfile`, `services/orchestrator/Dockerfile`, `apps/web/Dockerfile`
  - **Problema:** Falta de probes embutidos nas imagens para orquestradores que não utilizam compose.yaml.
  - **Ação:** Incluir instrução `HEALTHCHECK` consumindo endpoints `/health` ou `/ready`.

---

## 3. Persistência de Dados, Armazenamento S3 e Observabilidade

- [ ] **3.1 Criar rotina de backup automatizado do Postgres (pgvector)**
  - **Severidade:** Crítico
  - **Arquivos:** `infra/compose.yaml`, `scripts/backup-db.sh`
  - **Problema:** Não há rotina de backup do volume `pgdata`. Perda do volume resulta em perda irrecuperável de metadados, vetores e histórico.
  - **Ação:** Criar serviço sidecar ou cronjob com `pg_dump` periódico compactado e enviado para o S3 ou volume de backup dedicado.

- [ ] **3.2 Parametrizar credenciais do SeaweedFS (`seaweedfs-s3.json`) via variáveis de ambiente**
  - **Severidade:** Alto
  - **Arquivos:** `infra/seaweedfs-s3.json`, `infra/compose.yaml`
  - **Problema:** Credenciais e identidades S3 estão estáticas em arquivo de configuração sem interpolação de secrets do ambiente.
  - **Ação:** Fazer o `s3-init` gerar dinamicamente o arquivo `/etc/seaweedfs/s3.json` a partir de `S3_ACCESS_KEY` e `S3_SECRET_KEY` antes de iniciar o daemon S3.

- [ ] **3.3 Definir regras de Lifecycle e expiração de artefatos temporários no S3**
  - **Severidade:** Médio
  - **Arquivo:** `infra/seaweedfs-s3.json` / rotina no `manager`
  - **Problema:** Amostras intermediárias e uploads de treinos descartados acumulam no volume `seaweed_data` indefinidamente.
  - **Ação:** Configurar retenção/purga automática de artefatos não consolidados após 30 dias.

- [ ] **3.4 Implementar exportador de métricas (Prometheus) nos serviços Axum**
  - **Severidade:** Alto
  - **Arquivos:** `services/api-principal/src/main.rs`, `services/manager/src/main.rs`, `services/orchestrator/src/main.rs`
  - **Problema:** Ausência de endpoint `/metrics` para telemetria de conexões de banco, requisições HTTP, latência e fila de jobs.
  - **Ação:** Integrar `metrics-exporter-prometheus` nas rotas internas dos serviços.

---

## 4. Orquestração, Automação e Resiliência Operacional

- [ ] **4.1 Desacoplar IPs estáticos da configuração de nós GPU (TrueNAS)**
  - **Severidade:** Alto
  - **Arquivos:** `infra/compose.gpu.yaml:26-28`, `scripts/start-truenas.sh:22`
  - **Problema:** Endereços IP fixos (`10.15.10.3`, `10.15.1.2`) hardcoded nos arquivos de configuração inviabilizam portabilidade.
  - **Ação:** Utilizar nomes de host resolvíveis (mDNS, DNS interno ou VPN como Tailscale) e garantir variáveis com fallbacks genéricos no `.env`.

- [ ] **4.2 Adicionar limites de CPU e Memória nos serviços do Compose (`mem_limit`)**
  - **Severidade:** Alto
  - **Arquivo:** `infra/compose.yaml`
  - **Problema:** Falta de `deploy.resources.limits`. Um vazamento de memória pode derrubar o nó hospedeiro por OOM.
  - **Ação:** Configurar limites seguros de memória e CPU para cada serviço (`db`, `principal`, `manager`, `embedder`, `web`).

- [ ] **4.3 Ajustar ordem de subida e `depends_on` no `manager`**
  - **Severidade:** Médio
  - **Arquivo:** `infra/compose.yaml:129-131`
  - **Problema:** `manager` usa `condition: service_started` para `principal` e `db`, podendo iniciar antes do banco estar apto para receber conexões.
  - **Ação:** Configurar `condition: service_healthy`.

---

## 5. Pipeline de CI/CD

- [ ] **5.1 Incluir testes unitários do `manager` e `orchestrator` no CI**
  - **Severidade:** Crítico
  - **Arquivo:** `.gitea/workflows/ci.yml:68-74`
  - **Problema:** CI atual roda apenas `cargo test -p api-principal`. Quebras nos outros dois serviços passam despercebidas.
  - **Ação:** Atualizar para `cargo test --workspace`.

- [ ] **5.2 Adicionar validação de compilação e testes das Engines Python no CI**
  - **Severidade:** Alto
  - **Arquivo:** `.gitea/workflows/ci.yml`
  - **Problema:** Não há job de validação Python para as engines `trainer-yolo`, `trainer-difusao` e `trainer-clip`.
  - **Ação:** Adicionar etapa no workflow rodando `python -m compileall engines/*/src` e `uv run pytest`.

- [ ] **5.3 Otimizar caching e dependências do runner no CI**
  - **Severidade:** Médio
  - **Arquivo:** `.gitea/workflows/ci.yml`
  - **Problema:** O job reinstala `git`, `rustfmt` e dependências via `apt-get update` e `cargo` a cada execução.
  - **Ação:** Utilizar imagem base customizada para CI ou habilitar cache de dependências de Cargo e npm no runner.

---

## 6. Padronização e Portabilidade de Scripts Operacionais

- [ ] **6.1 Unificar scripts operacionais em CLI única (`scripts/heph.sh`)**
  - **Severidade:** Médio
  - **Arquivos:** `scripts/` (substituir fragmentação de `start-host*.sh`, `build-*.sh`, etc.)
  - **Problema:** Proliferação de 16 scripts isolados com funções sobrepostas gerando confusão operacional.
  - **Ação:** Consolidar comandos sob uma ferramenta unificada com subcomandos:
    - `heph up [--dev|--full|--gpu]`
    - `heph down`
    - `heph build [--host|--gpu|--all]`
    - `heph test [--unit|--smoke|--storage|--db]`
    - `heph reset`
    - `heph backup`

- [ ] **6.2 Criar script de diagnóstico pré-voo (`doctor.sh`)**
  - **Severidade:** Baixo
  - **Arquivo:** `scripts/doctor.sh`
  - **Problema:** Dificuldade em diagnosticar falhas de ambiente em novas máquinas.
  - **Ação:** Script para checar portas livres, suporte a Docker socket, drivers NVIDIA/ROCm, espaço em disco e ferramentas necessárias (`curl`, `openssl`, `jq`).
