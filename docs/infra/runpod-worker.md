# Nó Worker na RunPod (GPU Serverless/Hospedado)

Runbook para conectar um **pod GPU da RunPod** como worker de execução do
Hephaestus LLM Studio, usando o **MCP oficial da RunPod** para criar o template
por linguagem natural ou REST API.

---

## 1. Por que o worker da RunPod precisa de Docker-in-Docker

O `orchestrator` executa trainers efêmeros via `EXEC_MODE=docker`
(`docker run` no socket do host — ver `compose/remote-node.yaml`). Um pod
RunPod **é ele próprio um container**: não existe `/var/run/docker.sock` do
host dentro dele, e `EXEC_MODE=subprocess` é um stub no v1
(`executor_subprocess.rs` retorna "not supported").

A solução é a imagem **`infra/Dockerfile.runpod-worker`**:

- Estende `hephaestus-orchestrator:latest` (já traz `docker-cli` + binário).
- Instala `dockerd` + `containerd` + `iptables` + `nvidia-container-toolkit`
  configurado como runtime padrão do daemon interno.
- O entrypoint sobe o daemon interno, cria a rede bridge `heph-engine`
  (usada por `ENGINE_NETWORK`/`DIFFUSION_DAEMON_NETWORK`) e então executa o
  orchestrator. Os trainers-filho herdam a GPU pelo driver que a RunPod monta
  no pod.

> **Requisito:** o pod precisa ser criado em modo **PRIVILEGED** (tier
> verificado com a RunPod — o daemon interno não sobe sem isso; o entrypoint
> falha com mensagem explícita apontando a causa).

```bash
# Build & push (uma vez; a RunPod puxa do GHCR):
docker build -f infra/Dockerfile.runpod-worker \
  -t ghcr.io/felipecaninnovaes/hephaestus-runpod-worker:latest .
docker push ghcr.io/felipecaninnovaes/hephaestus-runpod-worker:latest
```

---

## 2. MCP da RunPod neste harness (omp)

O instalador guiado `npx @runpod/mcp-server@latest add` detecta Claude
Code/Desktop, Cursor, Windsurf e VS Code — **não conhece o omp**. A
configuração neste repo é direta em `.omp/mcp.json` (já adicionada):

```json
"runpod": {
  "type": "http",
  "url": "https://mcp.getrunpod.io/",
  "auth": { "type": "oauth" }
},
"runpod-docs": { "type": "http", "url": "https://docs.runpod.io/mcp" }
```

- **`runpod`** (hosted, OAuth "Sign in with Runpod"): gerencia pods, templates,
  volumes e endpoints. Nenhum segredo fica em disco. Após reiniciar a sessão,
  autorize via `/mcp` (abre o browser para login na console).
- **`runpod-docs`** (sem auth): busca na documentação oficial.
- Alternativa local (se preferir gerenciar a chave você mesmo): servidor
  `stdio` com `npx -y @runpod/mcp-server@latest` e `env.RUNPOD_API_KEY` —
  nesse caso coloque a chave **somente** no config de usuário (`~/.omp/...`),
  nunca no `.omp/mcp.json` rastreado pelo git (Regra 7 de `AGENTS.md`).

---

## 3. Gerar as credenciais do worker (Control Plane)

No host do Control Plane:

```bash
./scripts/heph.sh export-worker-env <IP_CONTROL_PLANE> <IP_DO_WORKER>
```

Copie do output: `MANAGER_TOKEN`, `S3_ORCH_SECRET_KEY` e `ORCH_PAIRING_CODE`.
Esses valores entram no template (ou como Runpod Secrets, formato
`{{ RUNPOD_SECRET_nome }}`).

### Conectividade obrigatória (bidirecional)

| Direção | Tráfego | Como resolver |
|:---|:---|:---|
| Worker → Control Plane | `MANAGER_URL :8081`, `S3_ORCH_ENDPOINT_URL :8333` | IPs de LAN **não** são alcançáveis pela RunPod. Use **Tailscale** (Control Plane como nó + subnet router) ou exponha o plano de controle publicamente atrás do Caddy com allowlist. |
| Control Plane → Worker | heartbeats/dispatch em `ORCH_ADVERTISE_URL :8082` | IP Tailscale do pod (`http://<ip-tailscale>:8082`) — estável entre restarts — ou o proxy HTTPS da RunPod (`https://<pod-id>-8082.rp.runpod.io`, muda a cada pod novo). |

---

## 4. Criar o template

### Via MCP (linguagem natural)

> "Crie um template Runpod privado `hephaestus-worker-gpu`, categoria NVIDIA,
> imagem `ghcr.io/felipecaninnovaes/hephaestus-runpod-worker:latest`, disco de
> container 40 GB, volume persistente 50 GB montado em `/data`, porta exposta
> `8082/http`, sem start command customizado, com estas variáveis de ambiente:
> `EXEC_MODE=docker`, `ORCH_WORKDIR=/data`, `ENGINE_NETWORK=heph-engine`,
> `DIFFUSION_DAEMON_NETWORK=heph-engine`, `DIFFUSION_DAEMON_ENABLED=1`,
> `DIFFUSION_DAEMON_PORT=8766`, `DIFFUSION_DAEMON_IDLE_TTL_S=600`,
> `ORCH_GPU_DEVICES=all`, `PORT=8082`, `HF_HOME=/data/models/huggingface`,
> `MANAGER_URL=http://<cp>:8081`,
> `S3_ORCH_ENDPOINT_URL=http://<cp>:8333`, `S3_ORCH_BUCKET=heph-data`,
> `S3_ORCH_ACCESS_KEY=heph-orch`, `S3_ORCH_SECRET_KEY=<segredo>`,
> `MANAGER_TOKEN=<token>`, `ORCH_PAIRING_CODE=<código>`,
> `ORCH_ADVERTISE_URL=http://<worker>:8082`."

### Via REST API (equivalente exato)

```bash
curl --request POST \
  --url https://rest.runpod.io/v1/templates \
  --header "Authorization: Bearer $RUNPOD_API_KEY" \
  --header 'Content-Type: application/json' \
  --data '{
  "category": "NVIDIA",
  "containerDiskInGb": 40,
  "dockerEntrypoint": [],
  "dockerStartCmd": [],
  "imageName": "ghcr.io/felipecaninnovaes/hephaestus-runpod-worker:latest",
  "isPublic": false,
  "isServerless": false,
  "name": "hephaestus-worker-gpu",
  "ports": ["8082/http"],
  "volumeInGb": 50,
  "volumeMountPath": "/data",
  "env": {
    "EXEC_MODE": "docker",
    "ORCH_WORKDIR": "/data",
    "ENGINE_NETWORK": "heph-engine",
    "DIFFUSION_DAEMON_NETWORK": "heph-engine",
    "DIFFUSION_DAEMON_ENABLED": "1",
    "DIFFUSION_DAEMON_PORT": "8766",
    "DIFFUSION_DAEMON_IDLE_TTL_S": "600",
    "ORCH_GPU_DEVICES": "all",
    "PORT": "8082",
    "HF_HOME": "/data/models/huggingface",
    "MANAGER_URL": "http://<cp>:8081",
    "S3_ORCH_ENDPOINT_URL": "http://<cp>:8333",
    "S3_ORCH_BUCKET": "heph-data",
    "S3_ORCH_ACCESS_KEY": "heph-orch",
    "S3_ORCH_SECRET_KEY": "<do export-worker-env>",
    "MANAGER_TOKEN": "<do export-worker-env>",
    "ORCH_PAIRING_CODE": "<do export-worker-env>",
    "ORCH_ADVERTISE_URL": "http://<worker>:8082"
  }
}'
```

### Via Console

`console.runpod.io → Templates → New Template`, com os mesmos campos da tabela
REST acima (Compute type NVIDIA; Storage 40 GB container + 50 GB volume em
`/data`; Network `8082/http`).

---

## 5. Subir o pod e adotar no Studio

1. **Novo Pod** a partir do template `hephaestus-worker-gpu`, GPU RTX 4090
   (24 GB) ou A100, com **privileged habilitado** (verificar disponibilidade
   do tier com a RunPod antes de escalar para produção).
2. Instale o Tailscale no boot do pod se usar a malha (opcional via start
   command extra, ou imagem com sidecar) — sem ele, use o proxy HTTPS.
3. Na UI do Studio: `/environments` → **Adotar Orquestrador** → Endpoint =
   `ORCH_ADVERTISE_URL` → tipo **Remoto** → cole o `ORCH_PAIRING_CODE`.
4. Verifique: o nó aparece com as GPUs e VRAM reportadas; dispare uma geração
   e observe o trainer efêmero subindo no daemon interno do pod
   (`docker ps` dentro do pod mostra `heph-diffusion-daemon` após o 1º job).

---

## 6. Custos e limitações

- **Billed while running:** pods RunPod cobram por segundo enquanto ligados;
  use `Stop` (não `Terminate`) para pausas — o volume (`/data`: cache HF,
  datasets, outputs) persiste entre restarts.
- **Primeiro job puxa a imagem do trainer** (`trainer-difusao:gpu` ~15 GB) do
  GHCR no daemon interno: adicione alguns minutos ao cold start; as imagens
  ficam no disco do container (40 GB) enquanto o pod viver.
- **`ORCH_PAIRING_CODE` é single-use**: para um 2º pod, gere um novo código com
  `export-worker-env` e crie um template derivado (ou sobrescreva a env no pod).
- **Privileged/DinD não é habilitado por default na RunPod** — confirme o tier
  suportado antes de automatizar a escala.
