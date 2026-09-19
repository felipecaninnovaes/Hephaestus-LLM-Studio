# Infraestrutura: Nós de Execução e GPU Remota

Guia sobre arquitetura de nós de execução, distribuição de imagens de motores, protocolo de pareamento, telemetria em tempo real, segurança de rede e operação de nós GPU dedicados (ex.: TrueNAS) no Hephaestus LLM Studio.

---

## 1. Topologia de Nós: Control Plane vs Data Plane

O Hephaestus separa o plano de controle da execução computacional pesada:

- **Control Plane (Dev Host — `10.15.10.3`):** Hospeda `api-principal` (BFF :8080), `manager` (:8081), `seaweedfs` (:8333), `db` (Postgres) e interface `web` (:3000).
- **Data Plane (Nó GPU Remoto — `10.15.1.2` TrueNAS):** Hospeda o `orchestrator-gpu` (:8082) e executa containers efêmeros de treino com acesso direto às GPUs físicas (ex.: RTX 3060 12 GB e GTX 1660 Super 6 GB).

```
       [ DEV HOST (10.15.10.3) ]                     [ TRUENAS / GPU WORKER (10.15.1.2) ]
  +-----------------------------------+          +------------------------------------------+
  | - Postgres (:5432)                |          |                                          |
  | - SeaweedFS S3 (:8333) [LAN]      | <======= | - orchestrator-gpu (:8082)               |
  | - Manager (:8081) [LAN]           |  Heart-  |   |-> GPU 0: RTX 3060 (12 GB) - Treino    |
  | - api-principal (:8080)           |  beat /  |   |-> GPU 1: GTX 1660S (6 GB) - Auxiliar |
  | - Web UI (:3000)                  |  Job API |   \-> /var/run/docker.sock               |
  +-----------------------------------+          +------------------------------------------+
```

---

## 2. Configuração de Rede do Dev Host e Riscos na LAN

Para que o nó TrueNAS consiga registrar heartbeats e baixar/subir dados no S3, o operador deve ajustar o arquivo `infra/.env` no dev host (`10.15.10.3`):

```bash
# infra/.env no dev host (10.15.10.3)
MANAGER_PUBLISH=0.0.0.0         # Libera a porta 8081 na interface de rede local
SEAWEED_PUBLISH=0.0.0.0         # Libera a porta 8333 na interface de rede local
S3_PUBLIC_ENDPOINT_URL=http://10.15.10.3:8333 # Endereço anunciado para presigned URLs
```

### Vetores de Exposição e Mitigações
- **Manager exposto em HTTP puro:** O manager valida chamadas via header `Authorization: Bearer <MANAGER_TOKEN>`. Sem criptografia TLS na LAN, esse token trafega em texto claro entre o TrueNAS e o dev host.
- **S3 em HTTP puro:** O tráfego de dados e imagens trafega aberto na rede local.
- **Mitigação Recomendada:**
  - Configurar regras de firewall (`ufw`) no dev host restringindo as portas `8081` e `8333` exclusivamente ao IP do TrueNAS (`10.15.1.2`):
    ```bash
    sudo ufw allow from 10.15.1.2 to any port 8081 proto tcp
    sudo ufw allow from 10.15.1.2 to any port 8333 proto tcp
    ```
  - Em ambientes corporativos ou não-confiáveis, utilizar um túnel WireGuard ou Tailscale unindo o dev host ao nó TrueNAS.

---

## 3. Como as Imagens de Treino Chegam ao Nó GPU

O `docker compose -f infra/compose.gpu.yaml up -d` sobe **apenas** o container do `orchestrator-gpu`. Ele **não** faz o download automático das imagens pesadas de treinamento (`trainer-yolo` e `trainer-difusao`).

Como o projeto não possui um registry privado configurado por padrão:
1. **Repositório Clonado no TrueNAS:** O código-fonte deve estar clonado diretamente no nó remoto (ex.: `/mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio`).
2. **Build Local Obrigatório das Imagens GPU:** Antes de despachar jobs reais, as imagens devem ser construídas localmente no TrueNAS utilizando os profiles dedicados de build:
   ```bash
   # Build da imagem YOLO GPU (~8 GB)
   docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build trainer-gpu
   
   # Build da imagem Difusão GPU (~15 GB)
   docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build trainer-difusao-gpu
   ```
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
   - Definido no `infra/env.gpu` do TrueNAS (ou gerado dinamicamente no boot do orchestrator).
   - O operador registra o nó na interface Web do Studio (ou via chamada POST ao manager) informando o código e o endereço anunciado (`ORCH_ADVERTISE_URL=http://10.15.1.2:8082`).
   - Após a validação do código, o manager cadastra o nó e estabelece a comunicação autenticada via HMAC/Bearer.
2. **Telemetria de VRAM via Heartbeat:**
   - A cada 5 segundos, o orchestrator executa o utilitário `nvidia-smi` no nó TrueNAS e envia ao manager:
     - Estado do nó (`idle` ou `busy`).
     - Lista de GPUs físicas, modelo, VRAM total (`max_gpu_mib`) e VRAM livre instantânea (`vram_free_mib`).
     - IDs dos jobs ativos.

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

## 6. Procedimento Operacional: Runbook TrueNAS (ADR-0010)

### 6.1 Pré-flight (Dev Host & TrueNAS)
```bash
# 1. Verificar manager e S3 no Dev Host (10.15.10.3)
curl -s http://10.15.10.3:8081/health
curl -s http://10.15.10.3:8333/

# 2. Verificar GPUs disponíveis no TrueNAS
ssh dockeruser@10.15.1.2 'nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv,noheader'
```

### 6.2 Preparar `infra/env.gpu` no TrueNAS
```bash
ssh dockeruser@10.15.1.2 'cat > /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio/infra/env.gpu <<EOF
MANAGER_TOKEN=<token-do-.env-do-dev-host>
S3_ORCH_ACCESS_KEY=<access-key>
S3_ORCH_SECRET_KEY=<secret-key>
S3_ORCH_BUCKET=heph-data
ORCH_GPU_DEVICES=0
ORCH_ADVERTISE_URL=http://10.15.1.2:8082
ORCH_PAIRING_CODE=heph_p_$(openssl rand -hex 8)
EOF'
```

### 6.3 Build das Imagens e Inicialização
```bash
# Build das imagens de motor (caso ainda não tenham sido geradas no nó)
ssh dockeruser@10.15.1.2 'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
  docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build trainer-gpu'

# Iniciar o orchestrator-gpu
ssh dockeruser@10.15.1.2 'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
  docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml up -d'
```

### 6.4 Finalização da Sessão
```bash
ssh dockeruser@10.15.1.2 'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
  docker compose -p gpu -f infra/compose.gpu.yaml down'
```
