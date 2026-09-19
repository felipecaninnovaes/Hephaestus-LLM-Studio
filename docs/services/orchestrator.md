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

## Varredura de Containers Órfãos no Boot (Sweep)

Reinicializações inesperadas da máquina host, falhas de energia ou crashes do orquestrador podem deixar containers de treino em execução descontrolada consumindo 100% da GPU.

Para eliminar esse cenário, o `orchestrator` implementa rotinas de higienização no boot:

1. **Sweep de Containers (`sweep_orphan_trainer_containers`):**
   - Na inicialização do processo, antes de aceitar qualquer requisição, executa uma varredura via Docker socket (`docker ps -q --filter name=trainer-`).
   - Todos os containers remanescentes com prefixo de treino são imediatamente encerrados e removidos.
2. **Sweep de Caches em Disco (`sweep_orphan_workdirs`):**
   - Inspeciona os subdiretórios de cache de datasets no diretório de trabalho (`workdir`).
   - Apaga dados temporários com mais de 24 horas de inatividade para prevenir esgotamento de espaço em disco no nó.

## Daemon de Difusão HTTP e Confiabilidade de Processos

Para possibilitar geração rápida e interativa de imagens via playground sem incorrer na latência de recarregar gigabytes de pesos na GPU a cada prompt:

- **Daemon em Standby:** O `orchestrator` gerencia um daemon local HTTP em Python que mantém o pipeline de difusão em memória VRAM.
- **TTL de Inatividade:** O daemon monitora o tempo ocioso através da variável `DIFFUSION_DAEMON_IDLE_TTL_S` (padrão 600 segundos). Ao expirar o tempo sem novas requisições de geração, o subprocesso descarrega os pesos e encerra graciosamente.
- **Terminação Forçada com `kill_on_drop`:**
  - Todo comando assíncrono ou subprocesso Tokio spawnado pelo `orchestrator` (seja o client Docker CLI ou o daemon de difusão) é configurado com `cmd.kill_on_drop(true)`.
  - Se a task assíncrona for cancelada (por exemplo, timeout ou abort solicitado pelo usuário), a destruição do handle do processo (`Drop`) emite imediatamente um sinal de encerramento (`SIGKILL`), impedindo a existência de processos zumbis na GPU.
