# Topologia de Rede e Gestão de VRAM

Este documento detalha o modelo de isolamento de rede dos serviços e a política de alocação de VRAM nos nós de processamento GPU.

## Topologia de Rede Docker

O ambiente do Hephaestus opera sobre uma rede bridge dedicada gerenciada pelo Docker Compose (`infra_default`):

```
+-------------------------------------------------------------+
| Rede Docker Bridge: infra_default                           |
|                                                             |
|  [web:3000] -> [principal:8080] -> [manager:8081]           |
|                       |                   |                 |
|                       v                   v                 |
|               [seaweedfs:8333]    [orchestrator:8082]       |
|                       ^                   |                 |
|                       |                   v                 |
|                 [db:5432]          [trainer-yolo/difusao]   |
+-------------------------------------------------------------+
   | (Host port)          | (Loopback only)
   v                      v
 Host: 8080/3000        127.0.0.1:5432, 127.0.0.1:8081, 127.0.0.1:8333
```

### Binds Seguros de Loopback

Para mitigar riscos de exposição em redes locais ou internet, os serviços de infraestrutura e serviços internos utilizam binds estritos em `127.0.0.1`:

- **Banco de Dados (PostgreSQL):** `${DB_PUBLISH:-127.0.0.1}:5432:5432`.
- **Object Storage (SeaweedFS S3):** `${SEAWEED_PUBLISH:-127.0.0.1}:8333:8333` e porta master em `127.0.0.1:9333`.
- **Manager:** `${MANAGER_PUBLISH:-127.0.0.1}:8081:8081`.
- **Orchestrator Local:** `${ORCHESTRATOR_PUBLISH:-127.0.0.1}:8082:8082`.
- **Embedder CLIP:** `127.0.0.1:8090:8090` (sem autenticação, acessível apenas localmente).

Apenas as portas públicas do frontend (`web` em `3000`) e do gateway (`api-principal` em `8080`) são liberadas para conexões externas necessárias ao usuário final.

## Políticas de VRAM e Alocação por Nó

A tomada de decisão para agendamento de jobs de treinamento e geração baseia-se em telemetria em tempo real e na tabela de políticas declarativa:

- **Fonte Canônica:** Todas as exigências mínimas de VRAM residem em `packages/policies/vram-table.yaml`.
- **Headroom de Segurança:** Por padrão, reserva-se um headroom (`headroom_gb: 2`) para o sistema operacional, desktop manager e drivers.
- **Margem de Medição:** Aplica-se `measure_margin: 1.25` sobre a memória reportada para acomodar picos durante backpropagation e fragmentação do PyTorch.

O `manager` recebe heartbeats periódicos de cada nó contendo a capacidade de VRAM (`max_gpu_mib` e `vram_free_mib`). Um job só sai do estado `queued` para `preparing` se existir ao menos um nó online com memória livre suficiente segundo as regras do modelo selecionado.

## Prevenção de Concorrência Destrutiva na GPU

Colisões de workloads pesados em GPU acarretam erros fatais de Out-Of-Memory (OOM) ou travamento do subsistema CUDA/ROCm. Para garantir estabilidade absoluta, aplicam-se três salvaguardas:

1. **Barreira de Job Único por Nó (Single-Job Mutex):**
   - O `orchestrator` impõe `max_concurrent_jobs: 1` por nó.
   - Enquanto um container de treinamento estiver em execução (`running`), nenhuma outra tarefa pesada é aceita naquele nó.
2. **Ciclo de Inatividade do Daemon de Difusão:**
   - O daemon interativo de geração rápida mantém os pesos carregados na GPU durante interações na UI, mas monitora o tempo ocioso via `DIFFUSION_DAEMON_IDLE_TTL_S` (padrão 600 segundos).
   - Ao receber um job de treino de difusão ou atingir o TTL, o daemon libera os recursos para evitar contenção de VRAM.
3. **Isolamento de Processos:**
   - Subprocessos e containers Docker executam com flags que garantem desalocação de recursos em caso de cancelamento da tarefa pai.
