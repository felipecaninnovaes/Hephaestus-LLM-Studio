# Hephaestus LLM Studio — Modelos de Deploy Compose

Este diretório contém os arquivos canônicos de implantação do Hephaestus LLM Studio utilizando imagens públicas pré-construídas do **GitHub Container Registry (`ghcr.io`)**.

---

## 1. Visão Geral dos 4 Modelos de Deploy

| Arquivo | Cenário de Uso | Serviços Subidos | Hardware Recomendado |
|:---|:---|:---|:---|
| **`local-com-local-node-gpu.yaml`** | Máquina única com GPU NVIDIA dedicada | Ingress (Caddy), Web UI, API BFF, Manager, DB (pgvector), SeaweedFS S3, Orchestrator GPU, Embedder | 16 GB+ RAM, GPU NVIDIA (RTX 3060 12GB+ recomendada) |
| **`local-com-local-node.yaml`** | Máquina única CPU / Mock / Desenvolvimento | Ingress, Web UI, API BFF, Manager, DB, S3, Orchestrator Local (CPU/Mock), Embedder | 8 GB+ RAM, CPU x86_64 |
| **`local-sem-node.yaml`** | Servidor Central / Control Plane Puro | Ingress, Web UI, API BFF, Manager (porta 8081 na LAN), DB, S3 (porta 8333 na LAN), Embedder | Mini PC, Servidor Doméstico, VPS sem GPU |
| **`remote-node.yaml`** | Worker Remoto de Execução GPU | Orchestrator GPU (porta 8082, socket docker montado) | TrueNAS SCALE, Servidor GPU secundário, RunPod |

---

## 2. Inicialização Rápida (Quickstart)

### Passo 1: Gerar credenciais seguras e variáveis de ambiente
Execute o assistente de inicialização na raiz do repositório:
```bash
./scripts/setup.sh
```
*(Ou copie manualmente o modelo: `cp compose/.env.example compose/.env` e ajuste os segredos).*

### Passo 2: Subir o modelo desejado

#### Cenário A: Instalação local com GPU NVIDIA
```bash
docker compose -f compose/local-com-local-node-gpu.yaml up -d
```

#### Cenário B: Instalação local em CPU / Testes
```bash
docker compose -f compose/local-com-local-node.yaml up -d
```

#### Cenário C: Servidor central com nós externos
No servidor central:
```bash
docker compose -f compose/local-sem-node.yaml up -d
```
No nó worker remoto com GPU (ex: TrueNAS):
1. Configure as variáveis `CONTROL_PLANE_IP`, `NODE_IP`, `MANAGER_TOKEN` e `ORCH_PAIRING_CODE` no `.env`.
2. Inicie o worker:
```bash
docker compose -f compose/remote-node.yaml up -d
```
3. Registre o nó na interface Web do Studio informando o IP e o código de pareamento.

---

## 3. Como os Motores (Trainers) são Executados

Os containers de treinamento (`trainer-yolo` e `trainer-difusao`) **não ficam rodando continuamente** no Docker Compose para evitar o consumo desnecessário de VRAM e memória.

O `orchestrator` utiliza o socket do Docker (`/var/run/docker.sock`) para instanciar containers efêmeros sob demanda quando um job é iniciado. O Docker do host baixa automaticamente as imagens necessárias do GHCR na primeira execução.
