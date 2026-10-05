# Infraestrutura: Nós de Execução e GPU Remota

Guia sobre arquitetura de nós de execução, distribuição de imagens de motores, protocolo de pareamento, telemetria em tempo real, segurança de rede e operação de nós GPU dedicados (ex.: VM Proxmox `docker-04`) no Hephaestus LLM Studio.

---

## 1. Topologia de Nós: Control Plane vs Data Plane

O Hephaestus separa o plano de controle da execução computacional pesada:

- **Control Plane (Dev Host — `10.15.10.3`):** Hospeda `api-principal` (BFF :8080), `manager` (:8081), `seaweedfs` (:8333), `db` (Postgres) e interface `web` (:3000).
- **Data Plane (Nó GPU Remoto — `10.15.50.114`, VM Proxmox dedicada `docker-04`, usuário `dockeruser` sem sudo):** Hospeda o `orchestrator-gpu` (:8082) e executa containers efêmeros de treino com acesso direto a 2 GPUs físicas, identificadas por UUID (estável a reboot — Pitfall D9; o índice é só entrada): GPU0 RTX 3060 12 GB `GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17` e GPU1 GTX 1660 Super 6 GB `GPU-c83cc056-07f7-d31e-cc98-7486ddac0296`. Cada job roda em uma placa (escolha manual por `gpuDevice` ou automática pelo manager — `docs/architecture/network-and-vram.md`); 1 job por nó. Disco: apenas 60 GB total (`/`), sem volumes extras montados — capacidade permanentemente restrita, exige build de uma imagem GPU por vez e prune agressivo entre builds.

```
       [ DEV HOST (10.15.10.3) ]                     [ VM GPU DEDICADA / docker-04 (10.15.50.114) ]
  +-----------------------------------+          +------------------------------------------+
  | - Postgres (:5432)                |          |                                          |
  | - SeaweedFS S3 (:8333) [LAN]      | <======= | - orchestrator-gpu (:8082)               |
  | - Manager (:8081) [LAN]           |  Heart-  |   |-> GPU 0: RTX 3060 (12 GB) + daemon    |
  | - api-principal (:8080)           |  beat /  |   |-> GPU 1: GTX 1660S (6 GB)             |
  | - Web UI (:3000)                  |  Job API |   \-> /var/run/docker.sock               |
  +-----------------------------------+          +------------------------------------------+
```

---

## 2. Configuração de Rede do Dev Host e Riscos na LAN

Para que o nó GPU remoto consiga registrar heartbeats e baixar/subir dados no S3, o operador deve ajustar o arquivo `infra/.env` no dev host (`10.15.10.3`):

```bash
# infra/.env no dev host (10.15.10.3)
MANAGER_PUBLISH=0.0.0.0         # Libera a porta 8081 na interface de rede local
SEAWEED_PUBLISH=0.0.0.0         # Libera a porta 8333 na interface de rede local
S3_PUBLIC_ENDPOINT_URL=http://10.15.10.3:8333 # Endereço anunciado para presigned URLs
```

### Vetores de Exposição e Mitigações
- **Manager exposto em HTTP puro:** O manager valida chamadas via header `Authorization: Bearer <MANAGER_TOKEN>`. Sem criptografia TLS na LAN, esse token trafega em texto claro entre o nó GPU e o dev host.
- **S3 em HTTP puro:** O tráfego de dados e imagens trafega aberto na rede local.
- **Mitigação Recomendada:**
  - Configurar regras de firewall (`ufw`) no dev host restringindo as portas `8081` e `8333` exclusivamente ao IP do nó GPU (`10.15.50.114`):
    ```bash
    sudo ufw allow from 10.15.50.114 to any port 8081 proto tcp
    sudo ufw allow from 10.15.50.114 to any port 8333 proto tcp
    ```
  - Em ambientes corporativos ou não-confiáveis, utilizar um túnel WireGuard ou Tailscale unindo o dev host ao nó remoto.

---

## 3. Como as Imagens de Treino Chegam ao Nó GPU

O `docker compose -f infra/compose.gpu.yaml up -d` sobe **apenas** o container do `orchestrator-gpu`. Ele **não** faz o download automático das imagens pesadas de treinamento (`trainer-yolo` e `trainer-difusao`).

Como o projeto não possui um registry privado configurado por padrão (os pacotes `ghcr.io` são privados e não há credencial disponível no nó):
1. **Repositório Clonado no Nó GPU:** O código-fonte deve estar clonado diretamente no nó remoto (ex.: `~/Hephaestus-LLM-Studio`).
2. **Build Local Obrigatório das Imagens GPU:** Antes de despachar jobs reais, as imagens devem ser construídas localmente no nó. Os trainers (`trainer-gpu`, `trainer-difusao-gpu`) herdam da base compartilhada `engines/base-gpu/Dockerfile` (`hephaestus/engine-base-gpu:0.1.0`: PyTorch 2.6.0 + CUDA 12.4 fixado por digest, libs apt do OpenCV, usuário `studio` uid 1000), injetada via `build.additional_contexts: base-gpu: service:engine-base-gpu` no `infra/compose.gpu.yaml` — exige **Docker Compose ≥ 2.20** e Buildx. O caminho canônico é o script, que constrói a base antes de qualquer trainer:
   ```bash
   ./scripts/build-gpu.sh               # base + orchestrator-gpu + trainer-gpu + trainer-difusao-gpu
   ./scripts/build-gpu.sh yolo          # base + trainer-gpu (hephaestus/trainer-yolo:gpu)
   ./scripts/build-gpu.sh difusao       # base + trainer-difusao-gpu (hephaestus/trainer-difusao:gpu)
   ./scripts/build-gpu.sh orchestrator  # só orchestrator-gpu
   ```
   Equivalente manual: `docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build engine-base-gpu` e depois o(s) trainer(s). O `diffusers` do `trainer-difusao` é fixado por SHA de commit (`ARG DIFFUSERS_COMMIT_SHA` em `engines/trainer-difusao/Dockerfile.gpu`); atualizar a versão = trocar o SHA. Disco de 60 GB: a base é comum às duas imagens, mas evite builds simultâneos.
3. **Alinhamento de Tags no Dev Host:**
   No arquivo `infra/.env` do dev host, o manager deve ser configurado para apontar para a tag de imagem construída no nó remoto:
   ```bash
   TRAINER_IMAGE=hephaestus/trainer-yolo:gpu
   DIFFUSION_TRAINER_IMAGE=hephaestus/trainer-difusao:gpu
   ```
   Após alterar, recriar o container do manager no dev host:
   ```bash
   docker compose -f infra/compose.yaml up -d manager --force-recreate
   ```

---

## 4. Protocolo de Pareamento e Heartbeat

1. **Código de Pareamento de Uso Único (`ORCH_PAIRING_CODE`):**
   - Definido no `infra/env.gpu` do nó GPU (ou gerado dinamicamente no boot do orchestrator).
   - O operador registra o nó na interface Web do Studio (ou via chamada POST ao manager) informando o código e o endereço anunciado (`ORCH_ADVERTISE_URL=http://10.15.50.114:8082`).
   - Após a validação do código, o manager cadastra o nó e estabelece a comunicação autenticada via HMAC/Bearer.
2. **Telemetria por placa via Heartbeat:**
   - A cada 5 segundos o orchestrator envia ao manager o estado do nó (`idle`/`busy`), os jobs ativos e `gpu_devices`: por GPU física `index`, `uuid`, `name`, VRAM total/usada (MiB), potência (W), utilização (%) e temperatura (°C). Sensor não suportado (`[N/A]`/`[Not Supported]`) vira ausente, nunca 0.
   - Fonte única: `GpuSampler` do orchestrator (`nvidia-smi --query-gpu=index,uuid,name,memory.total,memory.used,power.draw,utilization.gpu,temperature.gpu`, timeout 2s, cache compartilhado) lido pelo heartbeat e pelo coletor de métricas do job (`sys.gpu.util_pct|temp_c|power_w|vram_used_mb`).
   - O manager persiste `orchestrators.gpu_devices` (migration 0023; `vram_total_gb` = maior placa) e expõe `gpuDevices` em `GET /api/orchestrators` e na telemetria; a regra `vram_high` usa a placa do job.

---

## 5. Execução de Containers e Segurança do Docker Socket

O `orchestrator` utiliza a CLI do Docker diretamente via subprocessos em Rust (`tokio::process::Command::new("docker")`), comunicando-se com o daemon Docker do host através da montagem de volume `/var/run/docker.sock`:

- **Comandos executados pelo orchestrator:**
  - `docker run --name trainer-<engine>-<id> ...` (execução do job de treino)
  - `docker stop --time 5 <name>` (interrupção/abort)
  - `docker rm -f <name>` (limpeza de containers órfãos no boot)
  - `docker ps -q --filter name=trainer-` (varredura de containers órfãos)

### Nota Crítica sobre Proteção do Socket
- Proxies como `docker-socket-proxy` apenas validam endpoints HTTP e métodos (ex.: `POST /containers/create`). Eles **não inspecionam o corpo JSON** da requisição e, portanto, **não bloqueiam** montagens indevidas de diretórios do host (`HostConfig.Binds`) ou modo privilegiado se a criação de container estiver habilitada.
- **Mitigação Real:** Em workers remotos, isole a máquina em nível de rede e garanta que o usuário do daemon Docker (`dockeruser`) não possua privilégios de `sudo` no sistema operacional hospedeiro.

---

## 6. Procedimento Operacional: Runbook do Nó GPU (ADR-0010)

### 6.1 Pré-flight (Dev Host & Nó GPU)
```bash
# 1. Verificar manager e S3 no Dev Host (10.15.10.3)
curl -s http://10.15.10.3:8081/health
curl -s http://10.15.10.3:8333/

# 2. Verificar GPUs disponíveis no nó (docker-04)
ssh dockeruser@10.15.50.114 'nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv,noheader'
```

### 6.2 Preparar `infra/env.gpu` no nó GPU
```bash
ssh dockeruser@10.15.50.114 'cat > ~/Hephaestus-LLM-Studio/infra/env.gpu <<EOF
MANAGER_TOKEN=<token-do-.env-do-dev-host>
S3_ORCH_ACCESS_KEY=<access-key>
S3_ORCH_SECRET_KEY=<secret-key>
S3_ORCH_BUCKET=heph-data
ORCH_GPU_DEVICES=GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17
ORCH_ADVERTISE_URL=http://10.15.50.114:8082
ORCH_PAIRING_CODE=heph_p_$(openssl rand -hex 8)
DIFFUSION_DAEMON_ENABLED=1
DIFFUSION_DAEMON_GPU_DEVICE=GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17
EOF'
```

### 6.3 Build das Imagens e Inicialização
> **Nota de disco:** o nó tem só 60 GB de `/` — faça build de **uma imagem por vez** e rode `docker system prune -af` entre builds para evitar `no space left on device`.
```bash
# Build das imagens de motor (caso ainda não tenham sido geradas no nó)
ssh dockeruser@10.15.50.114 'cd ~/Hephaestus-LLM-Studio && \
  docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build trainer-gpu'

# Iniciar o orchestrator-gpu
ssh dockeruser@10.15.50.114 'cd ~/Hephaestus-LLM-Studio && \
  docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml up -d'
```

### 6.4 Finalização da Sessão
```bash
ssh dockeruser@10.15.50.114 'cd ~/Hephaestus-LLM-Studio && \
  docker compose -p gpu -f infra/compose.gpu.yaml down'
```

### 6.5 Dívida da Migração TrueNAS→docker-04: `trainer-yolo:gpu` Ausente
Na migração do antigo NAS TrueNAS para a VM dedicada `docker-04`, a imagem
`hephaestus/trainer-yolo:gpu` não foi reconstruída no nó novo (só
`trainer-difusao:gpu` havia sido migrada) — dívida órfã não documentada até
a fatia `feat/no-gpu-reuso-dataset-embeds`, onde travava o smoke de
invariante YOLO (item 3 do §6 da spec). Buildada nessa sessão via
`--profile build build trainer-gpu` (§6.3). Tags vigentes no nó após o
cutover `:reuso-cache` (W2 da mesma fatia): `gpu-orchestrator-gpu:latest`,
`hephaestus/trainer-difusao:gpu`, `hephaestus/trainer-yolo:gpu`; backups de
rollback preservados com sufixo `:pre-reuso-cutover` para as três imagens.

**Incidente 2026-10-03 (~00:50Z):** `hephaestus/trainer-difusao:gpu` e todas as tags de rollback antigas (`:pre-3a`, `:pre-metrickeys`, `:pre-telemetria`, `:gpu-pre-resume-fix`, `gpu-orchestrator-gpu:pre-multigpu-b1`) sumiram do nó; `trainer-yolo:gpu` foi recriada às 00:50:45Z. Causa não identificada (suspeita de prune externo); `trainer-difusao:gpu` rebuildada do build cache. Antes de depender de uma tag de rollback, conferir `docker image ls` no nó. Investigação em `tasks/backlog.md`.

### 6.6 Multi-GPU: seleção por UUID e daemon de difusão
- **`ORCH_GPU_DEVICES`** (UUID da 3060 no docker-04): só fallback — usado quando o dispatch chega sem `gpu_device` (nó/job legado). Com `gpu_device`, o executor roda `docker run --gpus "device=<UUID>"` e define `NVIDIA_VISIBLE_DEVICES=<UUID>` (as imagens CUDA trazem `=all`); `nvidia-smi -L` dentro do container lista só a placa do job.
- **`DIFFUSION_DAEMON_GPU_DEVICE`** (UUID da 3060): fixa o daemon de difusão numa placa. Job em outra GPU não derruba o daemon; job na mesma GPU (ou com GPU desconhecida) o derruba **só** se ocioso há mais de `DIFFUSION_DAEMON_IDLE_TTL_S/2` — usado há menos que isso, convive com o treino na mesma placa.
- **`DIFFUSION_DAEMON_ENABLED=1`** ligado no docker-04 em 2026-10-03 (backup `infra/env.gpu.bak-daemon`; default do `compose.gpu.yaml` é `0`). Geração quente em ~3 s; a 1ª geração com o daemon frio pode falhar enquanto ele baixa/carrega o modelo (backlog).
