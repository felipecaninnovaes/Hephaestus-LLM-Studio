# Especificação Técnica: Modularização, Desacoplamento e Autonomia do Orchestrator

**Status:** Especificação / Levantamento Aprovado (Read-Only, sem alteração de código ou restart de serviços)  
**Data:** 2026-09-18  
**Autor:** Hephaestus Architecture & Coordination  
**Alvo:** `services/orchestrator` (fronteiras com `services/manager`, `engines/` e `packages/contracts/openapi.yaml`)  
**Contexto Canônico:** `AGENTS.md` (Nível 0), `docs/services/orchestrator.md`, `docs/services/manager.md`, `docs/REPO_MAP.md`

---

## 1. Sumário Executivo e Diagnóstico de Complexidade

O serviço `orchestrator` (`:8082`) atua como o agente de execução de nós GPU no Hephaestus LLM Studio. Ele é stateless por design (sem conexão direta com PostgreSQL), operando sob ordens do `manager` via HTTP interno.

Apesar de sua arquitetura stateless correta, o crescimento acelerado de funcionalidades gerou uma concentração extrema de responsabilidades em arquivos monolíticos (*God Files*), forte acoplamento entre camadas de domínio e adaptadores de infraestrutura (Docker, S3, Axum), além de extensa duplicação de modelos de dados com o `manager` e testes inline massivos.

### 1.1 Métricas de Linhas de Código (LOC)

| Arquivo | Total LOC | LOC Produção | LOC Testes Inline (`#[cfg(test)]`) | % Testes | Problema Identificado |
|---|---|---|---|---|---|
| `services/orchestrator/src/lib.rs` | **7.975** | ~3.297 (41%) | **4.678 (59%)** | ~59% | Monólito congregando 15 responsabilidades distintas e mocks inline |
| `services/orchestrator/src/daemon.rs` | **1.140** | ~590 (52%) | **549 (48%)** | ~48% | Gestão de subprocessos, cliente HTTP, lifecycle e fakes misturados |
| `services/orchestrator/src/main.rs` | **726** | ~691 (95%) | ~33 (5%) | ~5% | Boot, 12+ parsers manuais de env, handlers Axum e loop de heartbeat |
| **Total Orchestrator** | **9.841** | **~4.578** | **~5.260** | **~53%** | **>50% do código do serviço são testes embutidos nos arquivos de prod** |

Para fins comparativos de fronteira:
- `services/manager/src/lib.rs` possui **4.610 LOC** (onde o dispatch é construído via JSON não-tipado).
- `services/manager/src/main.rs` possui **1.185 LOC** (com middlewares e helpers de erro repetidos).

### 1.2 Responsabilidades Acopladas Dentro de `lib.rs`
A autópsia detalhada identificou 15 domínios distintos dentro de um único arquivo `lib.rs`:
1. **Contratos e DTOs de Domínio:** `DispatchRequest`, `ReportBody`, `HeartbeatBody`, `WeightsRef`, `LoraRefStage`, etc. (L21–150).
2. **Identidade e Pareamento:** `PairingState`, geração de código single-use, advertise URL (L151–210).
3. **Erros e Guardas de Escopo:** `ScopedKeyError`, `PipelineError`, `GpuImageGuard` (L212–298).
4. **Isolamento de Chaves S3 (Barreira D2):** `S3Scope`, `scoped_key`, `scoped_init_image_key` (L299–387).
5. **Parsing de Métricas de Treino:** `MetricsLine`, `parse_metrics_line`, `to_report_json`, tailing (L389–599).
6. **Templating de Configuração:** `replace_config_placeholders`, `extract_epochs` (L597–696).
7. **Arquivos e Integridade:** `compute_file_md5`, `unzip_safe` contra zip-slip (L698–753).
8. **Porta e Cliente de Storage S3:** `S3Port` trait e `S3Client` com AWS SDK (L755–913).
9. **Cache Local de Pesos por Hash:** `stage_cached_weight` com hardlinks e staging atômico (L915–1013).
10. **Clientes HTTP de Retaguarda:** `HttpReportClient` e `HttpHeartbeatClient` (L1015–1130).
11. **Executores de Containers:** `DockerExecutor` (com montagem de args de GPU/shm) e `SubprocessExecutor` (L1132–1270).
12. **Higienização de Recursos (Sweeps no Boot):** `sweep_orphan_trainer_containers` e `sweep_orphan_workdirs` (L1271–1338).
13. **Estado de Concorrência de Jobs:** `ActiveJobs` com `DashMap` (L1363–1394).
14. **Pipeline Gigante de Execução:** `run_job` e `run_job_inner` com **mais de 1.500 linhas contínuas**, englobando download, unzips, múltiplos stagings de LoRA/pesos, forks de execução de daemon vs one-shot, uploads de artefatos com retry e envio de reports (L1433–3061).
15. **Telemetria de Host e GPU:** Coleta de CPU (`/proc/stat`), RAM (`/proc/meminfo`) e GPU via parse de CSV de `nvidia-smi` (L3110–3291).

---

## 2. Inventário de Duplicações e Acoplamento de Fronteira

A varredura entre `services/orchestrator`, `services/manager` e `services/api-principal` revelou duplicações severas de modelos, além de falta de tipagem segura na troca de mensagens internas.

### 2.1 Structs e Modelos Espelho Duplicados

| Conceito / Modelo | `orchestrator` | `manager` | Diagnóstico de Duplicação |
|---|---|---|---|
| **Pesos Base** | `WeightsRef { s3_key, md5 }` | `WeightsRef { s3_key, md5 }` | **100% idêntica**. Mesmos nomes, campos e derives. |
| **Checkpoint** | `WeightRef { s3_key, md5 }` | `ResolvedCheckpoint { s3_key, md5 }` | **100% idêntica** a `WeightsRef`. 3 structs para a mesma tupla `{ s3_key, md5 }`. |
| **Text Encoder** | `WeightRef { s3_key, md5 }` | `ResolvedTextEncoder { s3_key, md5 }` | **100% idêntica** a `WeightsRef`. |
| **Adaptador LoRA** | `LoraRefStage { s3_key, md5, scale }` | `ResolvedLora { s3_key, md5, scale }` | **100% idêntica**. Mesmos campos, tipos e finalidade com nomes divergentes. |
| **Imagem de Inicialização** | `InitImageRef { s3_key, md5 }` | `ResolvedInitImage { s3_key, md5 }` | **100% idêntica**. |
| **Referência de Pacote** | `PackageRef { key, md5_zip, bytes }` | `PackageRef` e `PreparePackageRef` | Campo `version_id` divergente no manager e ignorado pelo orchestrator. |
| **Requisição de Despacho** | `DispatchRequest` (fortemente tipada) | `serde_json::json!({ ... })` (não-tipado) | O manager monta o JSON dinamicamente sem usar struct tipada, gerando risco de quebra silenciosa no wire. |
| **Telemetria de GPU** | `GpuTelemetry { index, name, vram_total_mb, vram_free_mb }` | `NodeGpu { index, name, vram_total_bytes, vram_free_bytes }` | Mesmos dados com conversões manuais entre Megabytes e Bytes. |
| **Middlewares HTTP** | `request_id_middleware`, `error_response`, `bad_request`, `conflict` | Idênticos no `main.rs` do manager | Código Axum copiado e colado entre os dois serviços. |

### 2.2 Duplicações Internas no Orchestrator
- **Coleta de Imagens Geradas (`generated_*.png`):** A lógica de glob, thumbnail, detecção de metadados e deduplicação de nomes legados está duplicada entre o caminho do Daemon (`lib.rs:2149–2231`) e o caminho One-shot (`lib.rs:2730–2812`).
- **Staging de Pesos com Hash MD5:** O bloco de validação, download, cálculo de hash e hardlink é replicado 4 vezes em `run_job_inner` (para pesos base, loop de LoRAs, checkpoint customizado e text encoder).
- **Construção de Linha de Comando Docker:** Argumentos de rede, GPU (`--gpus`, `NVIDIA_VISIBLE_DEVICES`), volumes `/data/*` e shm são montados separadamente em `DockerExecutor` (`lib.rs:1163`) e em `DockerDaemonLauncher` (`daemon.rs:111`).

---

## 3. Diagnóstico do Daemon e Engines de Treinamento

A análise de `daemon.rs` e da execução de containers revelou os seguintes pontos críticos:

1. **Ilusão do `kill_on_drop` em Containers Docker:**  
   Em `SubprocessDaemonLauncher`, o processo do SO é filho direto do Tokio e `kill_on_drop(true)` envia SIGKILL ao processo Python. Contudo, em `DockerDaemonLauncher`, o comando executado é `docker run -d`. O `kill_on_drop` mata apenas o client CLI `docker` que já encerrou milissegundos após o spawn. O container real continua rodando de forma zumbi no Docker daemon se o orchestrator sofrer crash.
2. **Pooling Rudimentar com Retries Síncronos:**  
   Quando o daemon Python está ocupado gerando um batch (`409 Conflict`), o orchestrator executa um loop de retry com 3 tentativas e `tokio::time::sleep(500ms * attempt)`. Não há fila assíncrona (FIFO/MPSC) em memória, rejeitando requisições com `PipelineError::DaemonBusy` sob carga concorrente moderada.
3. **Preempção de VRAM Agressiva vs Descarte:**  
   A rotina `maybe_preempt_daemon` mata o daemon para liberar GPU para treinos somente se o daemon estiver ocioso por mais de metade do `idle_ttl`. Se o daemon estiver ocupado, não há coordenação cooperativa de encerramento.

---

## 4. Nova Arquitetura de Módulos (Hexagonal / Clean Architecture)

Para desacoplar as responsabilidades, viabilizar manutenções seguras e permitir commits atômicos (<300 LOC), a estrutura de `services/orchestrator/src/` deve ser reorganizada de acordo com a Clean Architecture estrita.

### 4.1 Árvore de Diretórios Proposta

```text
services/orchestrator/
├── Cargo.toml
├── Dockerfile
├── src/
│   ├── main.rs                   # Ponto de entrada enxuto: config -> wiring -> serve
│   ├── lib.rs                    # Fachada pública e re-exports (SEM lógica pesada)
│   │
│   ├── config/                   # Configuração e inicialização tipada
│   │   ├── mod.rs                # OrchestratorConfig unificado (validação fail-fast)
│   │   ├── env.rs                # Parsing seguro de variáveis com mascaramento de segredos
│   │   └── network.rs            # Unificação de ENGINE_NETWORK e DIFFUSION_DAEMON_NETWORK
│   │
│   ├── domain/                   # Camada Pura de Domínio (SEM dependência de Axum/Docker/AWS)
│   │   ├── mod.rs
│   │   ├── models.rs             # DispatchRequest, ReportBody, HeartbeatBody, WeightsRef, etc.
│   │   ├── errors.rs             # PipelineError, ScopedKeyError, AdmissionError
│   │   ├── metrics.rs            # Parsing de JSONL de métricas, cálculo de progresso
│   │   ├── template.rs           # Interpolação de config.yaml e extração de epochs
│   │   └── policy.rs             # Guardas de GPU (GpuImageGuard), limites de concorrência
│   │
│   ├── ports/                    # Interfaces e Contratos Abstratos (Traits Mockáveis)
│   │   ├── mod.rs
│   │   ├── storage.rs            # S3Port (get_file, put_file, ping)
│   │   ├── executor.rs           # TrainerExecutor (run, stop)
│   │   ├── reporter.rs           # ReportPort (envio de progresso e conclusão)
│   │   ├── heartbeat.rs          # HeartbeatPort (telemetria do nó para o cluster)
│   │   └── daemon.rs             # DaemonClient, DaemonLauncher
│   │
│   ├── adapters/                 # Adaptadores de Infraestrutura (Implementações Concretas)
│   │   ├── mod.rs
│   │   ├── storage_s3.rs         # S3Client usando aws-sdk-s3
│   │   ├── executor_docker.rs    # DockerExecutor e gerador unificado de docker_args
│   │   ├── executor_process.rs   # SubprocessExecutor para ambiente local/mock
│   │   ├── reporter_http.rs      # HttpReportClient (reqwest)
│   │   ├── heartbeat_http.rs     # HttpHeartbeatClient (reqwest)
│   │   ├── daemon_docker.rs      # DockerDaemonLauncher
│   │   ├── daemon_process.rs     # SubprocessDaemonLauncher
│   │   └── daemon_client.rs      # HttpDaemonClient
│   │
│   ├── app/                      # Casos de Uso e Orquestração de Pipelines
│   │   ├── mod.rs
│   │   ├── dispatch.rs           # Validação, admissão e disparo do job
│   │   ├── abort.rs              # Cancelamento e interrupção forçada
│   │   ├── run_job.rs            # Fluxo mestre do ciclo de vida do job
│   │   ├── stages/               # Estágios cirúrgicos do pipeline
│   │   │   ├── mod.rs
│   │   │   ├── download_pkg.rs   # Download do pacote de treino e unzip seguro
│   │   │   ├── stage_weights.rs  # Resolução unificada de pesos base, LoRA e encoders
│   │   │   ├── render_config.rs  # Geração do config.yaml de treino
│   │   │   ├── execution.rs      # Fork: Execução via Daemon vs One-shot Container
│   │   │   ├── artifact_collector.rs # Varredura unificada de imagens, samples e checkpoints
│   │   │   └── upload_output.rs  # Upload de artefatos com retry e envio do report final
│   │   └── outbox.rs             # Spool local durável para reports offline
│   │
│   ├── daemon/                   # Gerenciador de Ciclo de Vida do Daemon de Difusão
│   │   ├── mod.rs
│   │   ├── state.rs              # DaemonState (URL, status, locks de inferência)
│   │   ├── supervisor.rs         # Polling de prontidão e watchdog de timeout
│   │   ├── housekeeping.rs       # Task periódica de shutdown por inatividade (idle TTL)
│   │   └── preemption.rs         # Liberação sob demanda de VRAM para treinos
│   │
│   ├── storage/                  # Gestão de Disco Local e Caches
│   │   ├── mod.rs
│   │   ├── layout.rs             # Definição e criação de paths (/data/{datasets,outputs,tmp})
│   │   ├── cache_weights.rs      # Cache imutável de pesos baseado em MD5 e hardlinks
│   │   ├── archive.rs            # Extração segura de ZIP e cálculo de hashes
│   │   └── gc.rs                 # Limpeza periódica por watermark de espaço livre
│   │
│   ├── telemetry/                # Coleta de Métricas do Host e Aceleradores
│   │   ├── mod.rs
│   │   ├── host.rs               # Leitura de CPU (/proc/stat) e RAM (/proc/meminfo)
│   │   └── gpu.rs                # Execução e parsing resiliente de nvidia-smi
│   │
│   ├── security/                 # Barreira de Isolamento e Autenticação
│   │   ├── mod.rs
│   │   ├── scoped_keys.rs        # Restrição de acesso S3 por prefixo de job/dataset
│   │   └── pairing.rs            # Máquina de estado do código de pareamento single-use
│   │
│   ├── server/                   # Camada HTTP e Rotas Axum
│   │   ├── mod.rs
│   │   ├── router.rs             # Construção do axum::Router e injeção de estado
│   │   ├── state.rs              # AppState unificado
│   │   ├── handlers/             # Handlers HTTP isolados
│   │   │   ├── mod.rs
│   │   │   ├── health.rs         # GET /health, GET /ready, GET /metrics
│   │   │   ├── dispatch.rs       # POST /internal/dispatch
│   │   │   ├── abort.rs          # POST /internal/abort
│   │   │   └── pairing.rs        # POST /internal/pairing/verify
│   │   └── middleware/           # Middlewares Axum
│   │       ├── mod.rs
│   │       ├── auth.rs           # Validação Bearer MANAGER_TOKEN
│   │       └── request_id.rs     # Rastreabilidade x-request-id
│   │
│   └── testkit/                  # Suíte de Fakes Compartilhados (Apenas #[cfg(test)])
│       ├── mod.rs
│       ├── fake_s3.rs            # FakeS3Port em memória
│       ├── fake_executor.rs      # FakeTrainerExecutor
│       ├── fake_reporter.rs      # FakeReportPort gravando histórico em Vec
│       └── fixtures.rs           # Geradores de payloads canônicos de teste
│
└── tests/                        # Testes de Integração Desacoplados de src/
    ├── dispatch_lifecycle.rs     # Ciclo ponta a ponta com fakes
    ├── weights_cache.rs          # Testes de concorrência e hardlinks
    ├── artifact_collection.rs    # Coleta de glob e deduplicação de imagens
    └── daemon_lifecycle.rs       # Testes de prontidão, timeout e preempção
```

### 4.2 Princípios de Boundaries e Dependências
1. **Regra de Dependência Hexagonal:**  
   `domain` não depende de nenhuma biblioteca de I/O (`tokio`, `aws-sdk-s3`, `docker`, `reqwest`, `axum`).  
   `ports` define apenas as interfaces requeridas pelos casos de uso.  
   `adapters` e `server` dependem de `domain` e `ports`, nunca o inverso.
2. **Isolamento de Contratos de Rede:**  
   Nenhum serviço acessa banco de dados do outro; o orchestrator permanece 100% stateless com relação ao PostgreSQL do cluster.
3. **Mapeamento Explícito de Engines:**  
   A adição de um novo modelo ou motor de execução (ex: Whisper, LLM Fine-Tuning) exige tocar apenas em uma variante de `stages/execution.rs` e `stages/artifact_collector.rs`, eliminando a necessidade de alterar 8 pontos dispersos no código.

---

## 5. Plano de Autonomia e Independência do Nó

Para viabilizar a autonomia do nó de orquestração (capacidade de continuar operando ou recuperando-se com robustez mesmo diante de falhas de rede com o manager):

### 5.1 Outbox Durável de Relatórios (`app/outbox.rs`)
- **Problema Atual:** Se o `manager` estiver instável ou indisponível quando um job termina, o envio de `ReportBody` final falha após 3 retries rápidos e o resultado é descartado. O job fica preso em `running` no manager para sempre, e a imagem gerada não aparece na galeria.
- **Solução de Autonomia:** Implementação de spool em disco (`$ORCH_WORKDIR/.outbox/<job_id>_<seq>.json`).
  - O relatório final é persistido em arquivo local antes do envio HTTP.
  - Uma task assíncrona em background consome a outbox com backoff exponencial.
  - O manager processa os relatórios com idempotência baseada em `clientSeq`.

### 5.2 Heartbeat com Backoff Inteligente e Jitter (`telemetry/`)
- **Problema Atual:** O loop de heartbeat dispara a cada 2 segundos cravados. Se o manager oscilar, o nó gera centenas de erros no log e satura a rede.
- **Solução de Autonomia:** Ciclo adaptativo: 2s quando saudável; após 3 falhas consecutivas, recua gradualmente (4s, 8s, 16s até teto de 30s) com jitter aleatório para evitar *thundering herd* quando o manager reiniciar.

### 5.3 Auto-Recuperação e Reaper Periódico de Processos (`app/sweeper.rs`)
- **Problema Atual:** O sweep de containers e workdirs órfãos só roda 1 vez no boot do orchestrator.
- **Solução de Autonomia:** Supervisor periódico (a cada 60s):
  - Inspeciona containers Docker com prefixo `trainer-*` ativos no host e reconcilia com a tabela `ActiveJobs` em memória.
  - Containers sem job ativo associado no orchestrator por mais de 5 minutos são finalizados (`docker stop`), prevenindo vazamento de VRAM por tarefas abortadas externamente.

### 5.4 Garbage Collection (GC) de Disco por Watermark (`storage/gc.rs`)
- **Problema Atual:** Apenas pastas `datasets-cache` com mais de 24h são apagadas. A pasta `.weights-cache` cresce indefinidamente até esgotar o disco do nó (`ENOSPC`).
- **Solução de Autonomia:** GC baseado em marca d'água de espaço livre (ex: manter no mínimo 20 GB livres). Se o espaço livre cair abaixo do limite, aplica política LRU (*Least Recently Used*) no cache de pesos e descarta diretórios `tmp/` antigos.

### 5.5 Encerramento Gracioso (*Graceful Shutdown*)
- Interceptação de `SIGTERM` e `SIGINT` no `main.rs`:
  1. Marca o nó como indisponível para novos dispatches (`ready = false`).
  2. Notifica o daemon de difusão para descarregar a VRAM (`POST /shutdown`).
  3. Concede até 10 segundos para jobs em andamento finalizarem ou emite `docker stop --time 5` para evitar containers abandonados na GPU.
  4. Realiza o flush da outbox antes de encerrar o processo.

---

## 6. Classificação de Melhorias (P0 a P3)

Todas as melhorias levantadas foram classificadas por urgência, impacto arquitetural e complexidade de implementação.

| ID | Nível | Melhoria | Justificativa | Impacto Esperado |
|---|---|---|---|---|
| **P0-1** | **P0** | **Decomposição do `lib.rs` e extração de `testkit/`** | Arquivo de ~8k linhas impede revisão de código, dificulta ownership e torna qualquer refatoração arriscada. | Redução de 70% no acoplamento; PRs atômicos (<300 LOC) passam a ser possíveis. |
| **P0-2** | **P0** | **Outbox Durável de Reports Locais** | Perda de reports quando o manager cai deixa jobs zumbis e causa perda de outputs gerados. | Garantia de entrega *at-least-once* de status de término sem jobs presos no cluster. |
| **P0-3** | **P0** | **Eliminação de Corrida TOCTOU na Admissão** | Checagem de concorrência (`active.len() >= max`) separada da inserção permite aceitar múltiplos jobs concorrentes na GPU. | Eliminação de estouros de VRAM por sobreposição indevida de jobs de treino. |
| **P0-4** | **P0** | **Unificação de Coleta de Artefatos (`ArtifactCollector`)** | Globs de imagens, thumbs e metadados estão triplicados entre caminhos de execução. | Fim de divergências na persistência de imagens entre treinos e geração interativa. |
| **P0-5** | **P0** | **Unificação de Argumentos Docker (`adapters/docker_args.rs`)** | Montagem de flags de rede, volumes e GPU repetida em `DockerExecutor` e `daemon.rs`. | Ponto único de verdade para configuração de isolamento de containers e rede interna. |
| **P1-1** | **P1** | **Configuração Tipada com Validação Fail-Fast (`config/`)** | Dezenas de leituras de variáveis de ambiente com `.unwrap_or()` e `.expect()` dispersas pelo `main.rs`. | Inicialização previsível; logs limpos sem exposição de tokens ou segredos. |
| **P1-2** | **P1** | **Fatiamento de `run_job_inner` em Estágios (`stages/`)** | Função com mais de 1.500 linhas impossibilita testes unitários direcionados. | Cada estágio (download, staging, execução, upload) passa a ter testes isolados. |
| **P1-3** | **P1** | **Staging Unificado de Pesos (`stage_ref`)** | Quatro blocos quase idênticos de download e hardlink para base, LoRA, checkpoint e text encoder. | Redução de mais de 200 linhas de código redundante de staging. |
| **P1-4** | **P1** | **Extração da Camada HTTP do `main.rs` (`server/`)** | Handlers de API, router e middlewares misturados com o ciclo de vida do processo. | `main.rs` reduzido para <150 linhas focado apenas em inicialização e shutdown. |
| **P1-5** | **P1** | **Isolamento do Ciclo de Vida do Daemon (`daemon/`)** | Gerenciamento de TTL e preempção de VRAM emaranhados com a execução de jobs. | Políticas de VRAM e daemon testáveis sem necessidade de instanciar o pipeline de treino. |
| **P1-6** | **P1** | **Consolidação do `testkit/`** | 4 implementações parciais de `FakeS3`, `FakeReport` e `FakeExecutor` espalhadas nos testes inline. | Eliminação de ~1.200 linhas de código de testes duplicado e mocks divergentes. |
| **P2-1** | **P2** | **Heartbeat com Backoff Exponencial e Jitter** | Loop rígido de 2s gera tempestade de requisições e polui logs em momentos de instabilidade do manager. | Resiliência do nó sem sobrecarregar a rede do cluster. |
| **P2-2** | **P2** | **Reaper Periódico de Containers Órfãos** | Quedas inesperadas de processos deixam containers consumindo 100% de GPU sem monitoramento. | Recuperação autônoma de recursos do nó sem intervenção manual ou reinicialização. |
| **P2-3** | **P2** | **Healthcheck Local Rico e Isolado (`GET /ready`)** | Verificação de prontidão atual testa apenas ping no S3, ignorando o estado do Docker e do disco. | Diagnóstico preciso do estado real de prontidão do hardware do nó. |
| **P2-4** | **P2** | **GC de Cache de Disco por Watermark (`storage/gc.rs`)** | Diretório `.weights-cache` acumula arquivos até encher o disco da máquina. | Prevenção de falhas de I/O por esgotamento de partição. |
| **P2-5** | **P2** | **Graceful Shutdown com Liberação de GPU** | Parada do processo via SIGTERM não envia sinal de desligamento ao daemon nem aguarda containers. | Término limpo de tarefas sem processos zumbis persistindo na placa de vídeo. |
| **P3-1** | **P3** | **Remoção de Bypasses Silenciosos de Autenticação** | Ausência de `MANAGER_TOKEN` faz o middleware liberar requisições sem emitir alerta explícito. | Segurança reforçada com modo de desenvolvimento declarado expressamente via configuração. |
| **P3-2** | **P3** | **Métricas Prometheus Enriquecidas** | Endpoint `/metrics` expõe apenas 4 métricas básicas. | Visibilidade operacional de taxa de acerto de cache de pesos, duração de jobs e fila outbox. |
| **P3-3** | **P3** | **Unificação de Variáveis de Rede (`network.rs`)** | `ENGINE_NETWORK` e `DIFFUSION_DAEMON_NETWORK` usam fallbacks diferentes. | Padronização e simplificação do compose e da infraestrutura de deployment. |

---

## 7. Roteiro de Implementação Incremental (Fatias Verticais Seguras)

Para garantir risco zero de quebra, todos os testes existentes (`cargo test -p orchestrator`) devem continuar passando a cada passo. Cada fatia deve ser implementada em branch própria (`feat/orchestrator-modularization-*`), com commits atômicos (<300 LOC) seguindo Conventional Commits.

```
Fatia 0 ──> Fatia 1 ──> Fatia 2 ──> Fatia 3 ──> Fatia 4 ──> Fatia 5 ──> Fatia 6 ──> Fatia 7 ──> Fatia 8
(Testkit)   (Config)    (Domain)    (Server)    (Daemon)    (Storage)   (Pipeline)  (Outbox)    (GC & Reaper)
```

1. **Fatia 0: Estruturação do `testkit/` e Linha de Base**  
   - Criar diretório `src/testkit/` e mover mocks inline (`FakeS3`, `FakeExecutor`, `FakeReportClient`) sem alterar testes.
   - Validar que a suíte de testes de regressão continua 100% verde.
2. **Fatia 1: Extração da Camada de Configuração (`config/`)**  
   - Mover leituras de variáveis de ambiente do `main.rs` para `config/mod.rs` tipado com validação no boot.
   - O `main.rs` passa a consumir `OrchestratorConfig`.
3. **Fatia 2: Extração de Modelos Puros de Domínio e Portas (`domain/` e `ports/`)**  
   - Mover DTOs (`DispatchRequest`, `ReportBody`, etc.) e erros para `domain/`.
   - Definir traits abstratas em `ports/` (`S3Port`, `TrainerExecutor`, `ReportClient`, `HeartbeatClient`).
   - `lib.rs` re-exporta as structs mantendo compatibilidade de imports.
4. **Fatia 3: Extração da Camada HTTP do `main.rs` (`server/`)**  
   - Mover handlers de API (`health`, `dispatch`, `abort`, `pairing`), middlewares e router para `server/`.
   - Reduzir `main.rs` para orquestração de boot e escuta de porta TCP.
5. **Fatia 4: Isolamento do Subsistema do Daemon de Difusão (`daemon/`)**  
   - Reorganizar `daemon.rs` em submódulo `daemon/` com separação de `state.rs`, `supervisor.rs` e `housekeeping.rs`.
   - Manter as assinaturas de `ensure_daemon_ready` e `maybe_preempt_daemon`.
6. **Fatia 5: Extração da Camada de Storage e Cache de Pesos (`storage/`)**  
   - Extrair `stage_cached_weight` e lógica de MD5/hardlinks para `storage/cache_weights.rs`.
   - Extrair `unzip_safe` para `storage/archive.rs`.
7. **Fatia 6: Fatiamento de Estágios de Pipeline e Unificação de Coleta (`app/stages/`)**  
   - Decompor `run_job_inner` nos módulos cirúrgicos de `stages/`.
   - Criar `ArtifactCollector` unificado substituindo a lógica duplicada de globs de imagem e metadados.
   - Unificar o staging de pesos em função genérica `stage_ref`.
8. **Fatia 7: Unificação de Adaptadores Docker (`adapters/docker_args.rs`)**  
   - Centralizar a montagem de argumentos CLI do Docker (volumes, GPUs, redes) usada por treinos e pelo daemon.
9. **Fatia 8: Autonomia — Outbox Durável de Reports e Backoff de Heartbeat**  
   - Adicionar mecanismo de outbox com persistência local em disco para reports.
   - Adicionar lógica de backoff exponencial e jitter no loop de heartbeat.
10. **Fatia 9: Autonomia — Reaper Contínuo, GC de Disco e Graceful Shutdown**  
    - Implementar supervisor periódico contra containers órfãos.
    - Implementar GC com marca d'água de espaço livre no cache de pesos.
    - Tratar sinais `SIGTERM`/`SIGINT` para liberação segura de recursos e flush de disco.

---

## 8. Verificação e Critérios de Aceitação da Modularização

- **Preservação de Contrato:** Nenhuma alteração incompatível no wire format HTTP (`packages/contracts/openapi.yaml`). Rotas `/internal/dispatch`, `/internal/abort`, `/internal/report` e `/internal/heartbeat` mantêm campos camelCase e semântica de status HTTP.
- **Tamanho Máximo por Arquivo:** Nenhum arquivo do serviço deve ultrapassar **400 linhas de código**.
- **Separação de Testes:** Suíte de testes unitários alocada prioritariamente no módulo `testkit/` ou no diretório externo `tests/`, eliminando a poluição de arquivos de produção.
- **Isolamento de Domínio:** `domain/` deve compilar sem referências a `tokio`, `aws-sdk-s3`, `docker` ou `axum`.
- **Integridade da Suíte:** `cargo check -p orchestrator` e `cargo test -p orchestrator` devem passar 100% sem erros a cada commit.