# Hephaestus LLM Studio ⚡

<div align="center">

**Studio open-source para dataset management, rotulagem, treino YOLO e geração/fine-tuning de difusão (FLUX.2, SDXL, LoRA).**

[![Docker Compose](https://img.shields.io/badge/Docker_Compose-v2-blue.svg)](https://docs.docker.com/compose/)
[![Next.js](https://img.shields.io/badge/Next.js-16_App_Router-black.svg)](https://nextjs.org/)
[![Rust](https://img.shields.io/badge/Rust-Axum_Tokio-orange.svg)](https://www.rust-lang.org/)
[![PyTorch](https://img.shields.io/badge/PyTorch-CUDA_12.4-red.svg)](https://pytorch.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[Quickstart](#-quickstart-rápido) •
[Modelos de Deploy](#-os-4-modelos-de-deploy) •
[Nós GPU Remotos](#-conectar-um-nó-gpu-remoto-ex-truenas) •
[Arquitetura](#-arquitetura-do-sistema) •
[CLI Operacional](#-cli-operacional-scriptshephsh)

</div>

---

## 🚀 Quickstart Rápido

### Opção 1: Instalação One-Line via `curl` (Sem Clonar o Repositório)

Em qualquer servidor **Ubuntu/Debian, Arch ou CentOS** com apenas Docker instalado:

```bash
curl -fsSL https://raw.githubusercontent.com/felipecaninnovaes/Hephaestus-LLM-Studio/main/scripts/install.sh | bash
```

> **Dica (Início Automático):** Para rodar sem perguntas e subir os containers imediatamente:
> ```bash
> curl -fsSL https://raw.githubusercontent.com/felipecaninnovaes/Hephaestus-LLM-Studio/main/scripts/install.sh | bash -s -- --auto --start
> ```

O instalador:
1. Verifica dependências (`docker`, `docker compose`, `openssl`, `curl`).
2. Detecta automaticamente se há **GPU NVIDIA** ou se é um host **CPU-only**.
3. Gera segredos criptográficos seguros (`.env` e credenciais S3).
4. Configura o modelo Compose ideal para o seu hardware.

---

### Opção 2: Instalação via Git (Desenvolvedores)

```bash
# 1. Clone o repositório
git clone https://github.com/felipecaninnovaes/Hephaestus-LLM-Studio.git
cd Hephaestus-LLM-Studio

# 2. Execute o assistente de onboarding
./scripts/setup.sh

# 3. Inicie o modelo recomendado pelo assistente
docker compose -f compose/local-sem-node.yaml up -d   # Se for servidor central / CPU
# ou
docker compose -f compose/local-com-local-node-gpu.yaml up -d  # Se tiver GPU NVIDIA local
```

Após iniciar, acesse a interface web em **`http://localhost`** (ou no IP da máquina na rede local: `http://<seu-ip>`).

---

## 📦 Os 4 Modelos de Deploy

O Hephaestus LLM Studio possui 4 modelos declarativos prontos para uso em `compose/`:

| Modelo | Cenário de Uso | Serviços Subidos | Hardware Mínimo |
|:---|:---|:---|:---|
| **`local-sem-node.yaml`** | **Servidor Central / Control Plane** *(Recomendado para VPS / Mini PC sem GPU)* | Ingress (Caddy :80), Web UI, BFF API, Manager (porta 8081 na LAN), S3 SeaweedFS (porta 8333 na LAN), DB Postgres + pgvector, Embedder CLIP | 4 GB+ RAM, CPU x86_64 |
| **`local-com-local-node-gpu.yaml`** | **Tudo-em-Um com GPU NVIDIA** *(Desktop gamer ou workstation única)* | Todos os serviços do Control Plane + Orquestrador Local com aceleração CUDA | 16 GB+ RAM, GPU NVIDIA (RTX 3060 12GB+) |
| **`local-com-local-node.yaml`** | **Máquina única CPU / Mock** *(Testes e desenvolvimento de interface)* | Todos os serviços locais com execução simulada (`mock`) | 8 GB+ RAM, CPU x86_64 |
| **`remote-node.yaml`** | **Worker Remoto de Execução GPU** *(TrueNAS SCALE, servidor GPU ou RunPod)* | Apenas o Orquestrador GPU conectado ao Docker Socket do host worker | GPU NVIDIA dedicada com drivers e nvidia-container-toolkit |

---

## 🌐 Conectar um Nó GPU Remoto (ex: TrueNAS)

O Hephaestus separa o **Plano de Controle** (interface, banco, storage S3 e fila de jobs) do **Plano de Dados** (computação pesada na GPU).

```
   [ NAVEGADOR DO OPERADOR ]
              │ HTTP :80
              ▼
   [ CONTROL PLANE (ex: 10.15.30.118) ]
   ├── Ingress Caddy (:80/:443) -> Web Next.js & BFF Rust
   ├── Postgres (pgvector)
   ├── Manager (:8081 na LAN)
   └── SeaweedFS S3 (:8333 na LAN)
              ▲
              │ Heartbeat (:8081) / Upload de Artefatos (S3 :8333)
              ▼
   [ WORKER GPU / TRUENAS (ex: 10.15.1.2) ]
   ├── Orchestrator GPU (:8082)
   ├── Telemetria VRAM em tempo real (nvidia-smi)
   └── Containers efêmeros de treino e difusão (FLUX.2 / YOLO)
```

### Passo 1: Exportar a Configuração no Servidor Central
No host do Control Plane, execute:
```bash
./scripts/heph.sh export-worker-env <IP_CONTROL_PLANE> <IP_DO_WORKER>
```
*Exemplo:* `./scripts/heph.sh export-worker-env 10.15.30.118 10.15.1.2`

### Passo 2: Iniciar o Worker no Nó GPU
Copie o conteúdo gerado para o nó worker (TrueNAS) em `compose/.env` ou `infra/env.gpu` e suba o orquestrador:
```bash
docker compose -f compose/remote-node.yaml up -d
# ou no TrueNAS:
docker compose -p gpu -f infra/compose.gpu.yaml --env-file infra/env.gpu up -d orchestrator-gpu
```

### Passo 3: Adotar o Nó na Web UI
1. Abra o Studio no navegador: `http://<IP_DO_CONTROL_PLANE>/environments`
2. Clique em **Adotar Orquestrador**.
3. Preencha o Endpoint (ex: `http://10.15.1.2:8082`), selecione **Remoto** e cole o `Código de Nó` gerado no Passo 1.
4. O nó conecta instantaneamente e reporta a VRAM em tempo real!

---

## 🛠️ Funcionalidades Principais

- **Gestão Canônica de Datasets:** Suporte a tarefas YOLO (detecção com classes e bounding boxes), Difusão e OpenCLIP com busca semântica vetorial integrada (pgvector).
- **Modo Proxy de Storage Seguro:** Imagens e miniaturas são servidas diretamente através do proxy Caddy (`/api/...`), eliminando problemas de CORS, portas de storage bloqueadas e falhas de `localhost`.
- **Geração por Difusão de Última Geração:** Suporte ao **FLUX.2 Klein 4B** (Destilado e Base) com quantização 4-bit NF4 (BitsAndBytes) para geração rápida em placas de 8 GB–12 GB de VRAM.
- **Treino de Detecção YOLO:** Suporte a YOLO11 (nano, medium, custom) com tracking e logs de métricas por época.
- **Telemetria em Tempo Real (SSE):** Monitoramento contínuo de VRAM livre, uso de GPU, status de amostragem de steps e download/upload de artefatos.

---

## 💻 CLI Operacional (`scripts/heph.sh`)

O repositório inclui a CLI unificada para gestão do ambiente:

```bash
# Iniciar serviços do Studio
./scripts/heph.sh up [--dev|--full|--gpu|--prod]

# Parar serviços
./scripts/heph.sh down

# Estado atual dos containers
./scripts/heph.sh status

# Exportar variáveis de pareamento para nós remotos (TrueNAS)
./scripts/heph.sh export-worker-env [IP_CONTROL_PLANE] [IP_WORKER]

# Diagnóstico pré-voo de portas e dependências
./scripts/heph.sh doctor

# Snapshot e backup do banco Postgres
./scripts/heph.sh backup
```

---

## 📋 Requisitos de Sistema

- **Servidor Central / Control Plane:**
  - SO: Linux (Ubuntu 22.04+, Debian 12+, Arch Linux)
  - CPU: x86_64 com 2+ núcleos
  - RAM: 4 GB mínimo (8 GB recomendado)
  - Docker 24+ & Docker Compose v2.20+
- **Nó Worker GPU (Para Treino / Difusão):**
  - GPU: NVIDIA com arquitetura Turing ou superior (GTX 1660, RTX 20/30/40, A100, etc.)
  - VRAM: 8 GB mínimo para inferência FLUX 4-bit / YOLO; 12 GB+ recomendado para fine-tuning LoRA
  - Driver NVIDIA atualizado (535+) & `nvidia-container-toolkit` instalado

---

## 📄 Licença

Distribuído sob a licença MIT. Consulte `LICENSE` para obter mais informações.
