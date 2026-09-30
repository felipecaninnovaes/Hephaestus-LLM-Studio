# Serviço: orchestrator

O `orchestrator` é o agente local de nó de computação do Hephaestus LLM Studio, operando na porta `:8082`.

## Papel e Responsabilidades

- **Executor de Nós de Computação:** Instalado em cada máquina (ou nó GPU) com acesso direto aos aceleradores de hardware (NVIDIA CUDA ou AMD ROCm).
- **Arquitetura Stateless:** Não possui conexão direta com o banco de dados PostgreSQL. Recebe instruções de trabalho via `POST /internal/dispatch` originadas pelo `manager` e comunica progresso e telemetria através de `POST /internal/report`.
- **Gerenciador de Ciclo de Vida Local:** Coordena download de pacotes de dados e pesos do S3, montagem de parâmetros de treino, inicialização de containers Docker e coleta contínua de logs e métricas.
- **Contratos:** Rotas e interfaces de comunicação com o cluster estão documentadas em `packages/contracts/openapi.yaml`.

## Execução de Containers de Treinamento

Ao receber um job de treinamento, o orchestrator instancia as engines especializadas:

- **Imagens OCI Oficiais:**
  - Treinamento YOLO: `hephaestus/trainer-yolo:local`
  - Treinamento Difusão: `hephaestus/trainer-difusao:local`
- **Configuração de Rede (`--network`):**
  - Os containers de treino são anexados explicitamente à rede Docker interna configurada por `ENGINE_NETWORK` (geralmente `${COMPOSE_PROJECT_NAME:-infra}_default`).
  - As engines comunicam-se com o endpoint S3 do SeaweedFS para download do dataset e upload dos checkpoints gerados sem expor nenhuma porta ao host.
- **Isolamento de Volumes:**
  - Monta diretórios dedicados de trabalho em `/data` (`/data/datasets`, `/data/models`, `/data/outputs`), permitindo reutilização de caches locais e persistência segura dos artefatos.

## Varredura e Limpeza de Containers Órfãos (Sweep & Reaper)

Reinicializações inesperadas da máquina host, falhas de energia ou crashes do orquestrador podem deixar containers de treino em execução descontrolada consumindo 100% da GPU.

O `orchestrator` implementa rotinas ativas de higienização tanto no boot quanto em runtime:

1. **Sweep no Boot (`sweep_orphan_trainer_containers`):**
   - Na inicialização do processo, antes de aceitar qualquer requisição, executa uma varredura via Docker socket (`docker ps -q --filter name=trainer-`).
   - Todos os containers remanescentes com prefixo de treino são imediatamente encerrados e removidos.
2. **Reaper Periódico em Runtime (`adapters/sweeper.rs`):**
   - Loop assíncrono em background executado a cada 60 segundos.
   - Reconcilia containers Docker ativos (`trainer-*`) contra o estado em memória (`active_jobs`).
   - Containers que não constam em `active_jobs` e possuem idade superior à tolerância de 300 segundos são considerados órfãos.
   - A parada é realizada graciosamente (`docker stop --time 5`) antes da remoção forçada (`docker rm --force`), prevenindo corrupção de artefatos em disco.
3. **Sweep de Caches em Disco (`sweep_orphan_workdirs`):**
   - Inspeciona os subdiretórios de cache de datasets no diretório de trabalho (`workdir`).
   - Apaga dados temporários com mais de 24 horas de inatividade para prevenir esgotamento de espaço em disco no nó.

## Spool Outbox Durável de Reports

Para garantir a entrega confiável de relatórios de conclusão, falha ou cancelamento de jobs mesmo durante indisponibilidades do manager ou interrupções de rede:

- **Persistência em Disco:** Reports de terminalidade são salvos atômica e confiavelmente em `$ORCH_WORKDIR/.outbox/<job_id>.json` (gravação temporária + `atomic rename`).
- **Drain em Background:** Uma task dedicada em background processa a pasta a cada 5 segundos, enviando relatórios pendentes para `POST /internal/report` do manager.
- **Semântica At-Least-Once:** Arquivos são removidos da outbox apenas após confirmação HTTP de sucesso (status 2xx) ou erro irrecuperável de cliente (4xx). Falhas de conexão (5xx, timeouts) mantêm os itens no spool para retry automático.
- **Flush no Shutdown:** Durante o encerramento do orchestrator, um flush final síncrono é disparado para descarregar o spool antes da saída.

## Encerramento Gracioso (Graceful Shutdown)

O serviço intercepta sinais `SIGTERM` e `SIGINT` (Ctrl+C) através de `with_graceful_shutdown` no servidor Axum, executando uma sequência ordenada de teardown:

1. **Parada de Ingress:** O servidor HTTP deixa de aceitar novas conexões e requisições no endpoint de dispatch.
2. **Cancelamento de Tasks em Background:** Disparo de sinal de cancelamento (`broadcast::Sender`) para loops de heartbeat, outbox drain e reaper periódico.
3. **Drenagem de Jobs Ativos:** Concede janela de tolerância de até 10 segundos para que jobs em execução concluam ou realizem checkpoints.
4. **Parada Segura de Containers:** Containers residuais recebem sinal de parada controlada (`docker stop`).
5. **Desligamento do Daemon de Difusão:** Encerramento explícito do subprocesso daemon Python, liberando imediatamente a memória VRAM.
6. **Flush Final da Outbox:** Drenagem de relatórios pendentes em disco antes do encerramento do processo.
## Daemon de Difusão HTTP e Confiabilidade de Processos

Para possibilitar geração rápida e interativa de imagens via playground sem incorrer na latência de recarregar gigabytes de pesos na GPU a cada prompt:

- **Daemon em Standby:** O `orchestrator` gerencia um daemon local HTTP em Python que mantém o pipeline de difusão em memória VRAM.
- **TTL de Inatividade:** O daemon monitora o tempo ocioso através da variável `DIFFUSION_DAEMON_IDLE_TTL_S` (padrão 600 segundos). Ao expirar o tempo sem novas requisições de geração, o subprocesso descarrega os pesos e encerra graciosamente.
- **Terminação Forçada com `kill_on_drop`:**
  - Todo comando assíncrono ou subprocesso Tokio spawnado pelo `orchestrator` (seja o client Docker CLI ou o daemon de difusão) é configurado com `cmd.kill_on_drop(true)`.
  - Se a task assíncrona for cancelada (por exemplo, timeout ou abort solicitado pelo usuário), a destruição do handle do processo (`Drop`) emite imediatamente um sinal de encerramento (`SIGKILL`), impedindo a existência de processos zumbis na GPU.
