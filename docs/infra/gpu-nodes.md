# Infraestrutura: Nós de Execução e GPU Remota

Guia sobre arquitetura de nós de execução, protocolo de pareamento, telemetria em tempo real e operação de nós GPU dedicados (ex.: TrueNAS / servidores remotos) no Hephaestus LLM Studio.

---

## 1. Topologia de Nós: Control Plane vs Data Plane

O Hephaestus separa o plano de controle da execução computacional pesada:

- **Control Plane (Dev Host):** Hospeda `api-principal` (BFF :8080), `manager` (:8081), `seaweedfs` (:8333), `db` (Postgres) e interface `web` (:3000).
- **Data Plane (Nós de Execução):** Hospeda instâncias do `orchestrator` (:8082). Pode rodar localmente no dev host (para CPU/mock) ou em máquinas dedicadas com aceleração GPU na rede local.

```
       [ DEV HOST (10.15.10.3) ]                     [ TRUENAS / GPU WORKER (10.15.1.2) ]
  +-----------------------------------+          +------------------------------------------+
  | - Postgres (:5432)                |          |                                          |
  | - SeaweedFS S3 (:8333)            | <======= | - orchestrator-gpu (:8082)               |
  | - Manager (:8081)                 |  Heart-  |   |-> GPU 0: RTX 3060 (12 GB) - Treino    |
  | - api-principal (:8080)           |  beat /  |   |-> GPU 1: GTX 1660S (6 GB) - Auxiliar |
  | - Web UI (:3000)                  |  Job API |   \-> /var/run/docker.sock               |
  +-----------------------------------+          +------------------------------------------+
```

---

## 2. Protocolo de Pareamento e Heartbeat

O `manager` gerencia o catálogo de nós autorizados a executar jobs de treinamento e inferência.

### 2.1 Modos de Pareamento
1. **Auto-Adopt Local (`AUTO_ADOPT_LOCAL=1`):**
   - Utilizado no `compose.yaml` de desenvolvimento diário.
   - O manager adota automaticamente o `orchestrator-local` sem intervenção do operador.
2. **Pareamento Seguro de Nós Remotos (`ORCH_PAIRING_CODE`):**
   - Para workers remotos (como o TrueNAS), o orquestrador gera (ou recebe via `env.gpu`) um código de uso único (`ORCH_PAIRING_CODE`).
   - O operador registra o nó na interface do Studio (ou via API do manager) informando o código e o endereço anunciado (`ORCH_ADVERTISE_URL=http://10.15.1.2:8082`).
   - As comunicações subsequentes utilizam assinaturas HMAC validadas pelo `MANAGER_TOKEN`.

### 2.2 Telemetria em Tempo Real (Heartbeats)
A cada intervalo regular (padrão de 5 segundos), o daemon do orchestrator envia um heartbeat para o manager contendo:
- **Estado do Nó:** `idle`, `busy` ou `offline`.
- **Inventário de GPUs:** Índice, nome do modelo, UUID e arquitetura.
- **Métricas de VRAM:** Memória total (`max_gpu_mib`) e memória livre em tempo real (`vram_free_mib`), obtidas diretamente via chamada ao utilitário `nvidia-smi`.
- **Jobs Ativos:** Identificadores e status das tarefas em processamento.

---

## 3. Configuração de Runtime e Isolamento GPU

O provisionamento no nó GPU é definido em `infra/compose.gpu.yaml`:

```yaml
services:
  orchestrator-gpu:
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: all
              capabilities: [gpu]
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - gpu_datasets:/data/datasets
      - gpu_outputs:/data/outputs
```

### 3.1 Alocação Seletiva de GPUs por Job
- O container do `orchestrator-gpu` recebe visibilidade de todas as GPUs (`count: all`) para coletar telemetria global.
- Quando o orchestrator executa um container de treinamento (ex.: `trainer-yolo` ou `trainer-difusao`), ele injeta a flag de dispositivo selecionado via `ORCH_GPU_DEVICES` (ex.: `--gpus device=0` para alocar apenas a RTX 3060).

### 3.2 Prevenção de Colisão de Workloads (Single-Job Mutex)
- Cada nó impõe `max_concurrent_jobs: 1`.
- Enquanto uma engine de treino estiver ativa, nenhuma outra carga pesada é despachada para o mesmo nó, eliminando o risco de erros fatais de Out-Of-Memory (OOM).

---

## 4. Ciclo de Vida do Daemon de Difusão

Para o pipeline de geração interativa no Studio:
- **Manutenção em VRAM:** Com `DIFFUSION_DAEMON_ENABLED=1`, os pesos do modelo (SD 1.5 / SDXL / FLUX) permanecem pré-carregados na GPU na porta interna `:8766`, permitindo latência na faixa de centenas de milissegundos para novas gerações.
- **Desalocação por Ociosidade:** O parâmetro `DIFFUSION_DAEMON_IDLE_TTL_S` (padrão 600 segundos) monitora a inatividade e encerra o processo do daemon se nenhuma requisição ocorrer, liberando a VRAM para tarefas de treinamento.

---

## 5. Procedimento Operacional: Sessão GPU no TrueNAS

O runbook completo reside em `infra/README-gpu.md`. O fluxo padrão de operação consiste em:

1. **Pre-flight no Dev Host:**
   - Conferir se `manager` (:8081) e `seaweedfs` (:8333) estão saudáveis e acessíveis na rede local:
     ```bash
     curl -s http://10.15.10.3:8081/health
     ```
2. **Conferir GPUs no TrueNAS via SSH:**
   ```bash
   ssh dockeruser@10.15.1.2 'nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv,noheader'
   ```
3. **Configurar e Iniciar Container no TrueNAS:**
   ```bash
   # Preparar infra/env.gpu com MANAGER_TOKEN, credenciais S3 e ORCH_PAIRING_CODE
   ssh dockeruser@10.15.1.2 'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && docker compose -p gpu -f infra/compose.gpu.yaml --env-file infra/env.gpu up -d'
   ```
4. **Finalização da Sessão:**
   ```bash
   ssh dockeruser@10.15.1.2 'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && docker compose -p gpu -f infra/compose.gpu.yaml down'
   ```
