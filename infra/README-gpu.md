# GPU Session Checklist — TrueNAS (ADR-0010)

Checklist operacional para sessão de treino real com GPU no TrueNAS.
**O G.6 (coordenador) é o primeiro usuário deste documento.**

- **Dev host:** `10.15.10.3` (roda Postgres, manager, principal, seaweedfs, orchestrator-local)
- **TrueNAS:** `10.15.1.2` (roda o orquestrador remoto com GPU — RTX 3060 + GTX 1660 Super)
- **Repo clonado no TrueNAS:** `/mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio`

> ⚠️ Todos os comandos SSH no TrueNAS usam `ssh dockeruser@10.15.1.2`.
> O TrueNAS **sem sudo**; containers rodam via grupo docker.

---

## 1. Pre-flight (dev host)

### 1.1 Conferir GPUs e VRAM no TrueNAS

```bash
ssh dockeruser@10.15.1.2 'nvidia-smi -L'
# Esperado:
# GPU 0: NVIDIA GeForce RTX 3060 (UUID: ...)
# GPU 1: NVIDIA GeForce GTX 1660 SUPER (UUID: ...)
```

```bash
ssh dockeruser@10.15.1.2 'nvidia-smi --query-gpu=index,memory.used,memory.total --format=csv,noheader,nounits'
# Exemplo saída:
# 0, 0, 12288     ← 3060 livre (OK, default)
# 1, 0, 6144      ← 1660S livre
```

> **Se a 3060 (GPU 0) estiver ocupada** (ex.: `graft deep` rodando),
> usar `ORCH_GPU_DEVICES=1` no `env.gpu`. O adopt (passo 2.5)
> detecta as GPUs automaticamente via heartbeat do orquestrador.

### 1.2 Verificar manager no dev host

```bash
curl -s http://10.15.10.3:8081/health
# Esperado: HTTP 200
```

### 1.3 Preparar `env.gpu` no TrueNAS

```bash
ssh dockeruser@10.15.1.2 \
  'cat > /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio/infra/env.gpu <<EOF
MANAGER_TOKEN=<token-do-.env-do-dev-host>
S3_ORCH_ACCESS_KEY=<access-key>
S3_ORCH_SECRET_KEY=<secret-key>
S3_ORCH_BUCKET=heph-data
ORCH_GPU_DEVICES=0
ORCH_ADVERTISE_URL=http://10.15.1.2:8082
ORCH_PAIRING_CODE=<código-de-pareamento-escolhido>
EOF'
```

> `ORCH_ADVERTISE_URL` deve ser o IP/porta **públicos** do TrueNAS
> (não hostname local — o manager do dev host usa este endereço para
> despachar jobs). `ORCH_PAIRING_CODE` é single-use e consumido pelo
> adopt.

### 1.4 Verificar S3 SeaweedFS no dev host

```bash
curl -s -o /dev/null -w '%{http_code}' http://10.15.10.3:8333/
# Esperado: 403 (AccessDenied — API no ar, identidade exige SigV4)
```

> Se S3 não responder na LAN, o `SEAWEED_PUBLISH` pode estar em
> loopback — verificar o `.env` do dev host.

---

## 2. Sessão (dev host) — ordem OBRIGATÓRIA (D2 ADR-0010)

### 2.1 Publicar S3 na LAN

```bash
# .env do dev host (infra/.env):
#   SEAWEED_PUBLISH=10.15.10.3
```

Edite `infra/.env` e adicione/atualize:
```
SEAWEED_PUBLISH=10.15.10.3
```

Recriar o SeaweedFS (bind-mount não detecta mudança de env — lição F4.6 #5):
```bash
docker compose -f infra/compose.yaml up -d seaweedfs --force-recreate
```

Verificar:
```bash
curl -s -o /dev/null -w '%{http_code}' http://10.15.10.3:8333/
# Esperado: 403 (API S3 respondendo na LAN)
```

### 2.2 Configurar imagem do trainer GPU

```bash
# .env do dev host (infra/.env):
#   TRAINER_IMAGE=hephaestus/trainer-yolo:gpu
```

Edite `infra/.env` e adicione/atualize:
```
TRAINER_IMAGE=hephaestus/trainer-yolo:gpu
```

Recriar o manager (ele lê `TRAINER_IMAGE` no boot):
```bash
docker compose -f infra/compose.yaml up -d manager --force-recreate
```

### 2.3 Desabilitar auto-adoção do orquestrador local

```bash
# .env do dev host (infra/.env):
#   AUTO_ADOPT_LOCAL=0
```

Edite `infra/.env` e adicione/atualize:
```
AUTO_ADOPT_LOCAL=0
```

Recriar o manager (ele lê `AUTO_ADOPT_LOCAL` no boot):
```bash
docker compose -f infra/compose.yaml up -d manager --force-recreate
```

> **Nota:** os passos 2.2 e 2.3 podem ser combinados em uma única edição
> do `.env` seguida de um único `--force-recreate` do manager.

### 2.4 Parar o orquestrador local

```bash
docker compose -f infra/compose.yaml stop orchestrator-local
```

> **Obrigatório** (D8 ADR-0010): o heartbeat sem identidade do manager
> sobrescreve o cache global a cada ~2s. Com o local parado, a telemetria
> reflete exclusivamente o TrueNAS.

### 2.5 Adotar o orquestrador remoto via API (fatia H)

O manager expõe `POST /api/orchestrators/adopt` que consome o
`ORCH_PAIRING_CODE` do orquestrador — não há mais necessidade de
acesso direto ao Postgres.

**Passo 1 — Revogar o orquestrador local:**

```bash
# Obter o ID do orquestrador local:
LOCAL_ID=$(curl -s http://10.15.10.3:8080/api/orchestrators \
  | python3 -c "import sys,json; [print(o['id']) for o in json.load(sys.stdin) if o['kind']=='local']")

# Revogar (DELETE):
curl -X POST "http://10.15.10.3:8080/api/orchestrators/${LOCAL_ID}/revoke"
# Ou: botão "Revogar" na página /environments
```

**Passo 2 — Adotar o orquestrador remoto:**

```bash
curl -X POST http://10.15.10.3:8080/api/orchestrators/adopt \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "orchestrator-gpu",
    "endpoint": "http://10.15.1.2:8082",
    "kind": "remoto",
    "pairingCode": "<código-do-env.gpu>"
  }'
# Ou: modal "Adotar" na UI
```

Verificar:
```bash
curl -s http://10.15.10.3:8080/api/orchestrators \
  | python3 -m json.tool
# Esperado: 1 row, kind='remoto', endpoint='http://10.15.1.2:8082', status='online'
```

---

## 3. TrueNAS — build e start

### 3.1 Atualizar o repo

```bash
ssh dockeruser@10.15.1.2 \
  'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && git pull'
```

### 3.2 Build da imagem do trainer GPU

```bash
ssh dockeruser@10.15.1.2 \
  'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
   docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml --profile build build trainer-gpu'
```

> Build ~8-10GB (PyTorch + CUDA + ultralytics + peso yolo11n baked).
> Tempo estimado: 5-15min dependendo da rede.

### 3.3 Subir o orquestrador GPU

```bash
ssh dockeruser@10.15.1.2 \
  'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
   docker compose -p gpu --env-file infra/env.gpu -f infra/compose.gpu.yaml up -d orchestrator-gpu'
```

### 3.4 Verificar saúde do orquestrador (do dev host)

```bash
curl -s http://10.15.1.2:8082/health
# Esperado: HTTP 200 com body {"status":"ok"}
```

### 3.5 Verificar telemetria de GPU

```bash
curl -s http://10.15.10.3:8080/api/telemetry | python3 -m json.tool | head -20
# Esperado: {measured, cpu, ram, ramTotal, vramUsed, vramTotal, gpus[], jobsActive}
# gpus[] com nomes reais, vramUsed/vramTotal > 0
```

---

## 4. Treino

### 4.1 Submit via UI

1. Acesse `http://10.15.10.3:3000` (studio web)
2. Faça login
3. Crie um dataset YOLO com classes e imagens via API:
   ```bash
   # Criar dataset (POST /api/datasets):
   curl -b cookies.txt -X POST http://10.15.10.3:8080/api/datasets \
     -H 'Content-Type: application/json' \
     -d '{"name":"meu-dataset","kind":"yolo","classes":["objeto1","objeto2"]}'
   # Upload de imagens: POST /api/datasets/{id}/upload
   # Anotar boxes: POST /api/datasets/{id}/boxes
   # Verificar elegibilidade YOLO: dataset kind=yolo com classes + imagens
   ```
4. Crie um novo job YOLO com:
   - Dataset: selecione o dataset YOLO criado
   - Modelo: `yolo11n`
   - Epochs: `3`
   - Batch: `16` (3060 com 12GB) ou `8` (1660S com 6GB)
   - Image size: `640`
5. Acompanhe em `/jobs` — o job deve ir para o orquestrador remoto

### 4.2 Submit via curl

```bash
# Login primeiro (obter cookie heph_session):
curl -c cookies.txt -X POST http://10.15.10.3:8080/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"password":"<STUDIO_PASSWORD>"}'

# Submeter job:
curl -b cookies.txt -X POST http://10.15.10.3:8080/api/jobs/yolo \
  -H 'Content-Type: application/json' \
  -d '{
    "datasetId": "<uuid-do-dataset>",
    "model": "yolo11n",
    "epochs": 3,
    "batch": 16,
    "imgsz": 640
  }'
```

> Acompanhe o progresso em `GET /api/jobs` ou via UI `/jobs`.
> O orquestrador remoto faz heartbeat a cada ~2s; a UI reflete
> `jobsActive` em tempo real.

---

## 5. Verificação binária (6 critérios G.6)

Após o job `done`, verificar **todos** os critérios:

### Critério 1 — Job foi para o remoto

```sql
-- No Postgres do dev host:
SELECT o.name, o.kind, j.orchestrator_id, j.status
FROM jobs j JOIN orchestrators o ON j.orchestrator_id = o.id
ORDER BY j.created_at DESC LIMIT 1;
-- Esperado: kind='remoto', status='done'
```

### Critério 2 — GPU correta foi usada

No TrueNAS, durante o run:
```bash
ssh dockeruser@10.15.1.2 'nvidia-smi'
# Esperado: processo do trainer rodando na GPU escolhida (util>0),
#            ~0 na outra
```

No log do container (após done):
```bash
ssh dockeruser@10.15.1.2 \
  'docker logs $(docker ps -q --filter "name=trainer-yolo-job" | head -1) 2>&1 | grep -i "using device"'
# Esperado: "Using device 0" (ou 1, conforme pre-flight)
```

### Critério 3 — Artefatos reais

```sql
-- Tamanho do best.pt deve ser > 1MB (não 110 bytes HEPHMOCK):
SELECT path, bytes FROM job_artifacts
WHERE job_id = '<uuid_do_job>' AND path LIKE '%best.pt';
-- Esperado: bytes > 1000000
```

Ou via API:
```bash
curl -s http://10.15.10.3:8080/api/jobs/<job_id> | python3 -c "
import sys, json; d=json.load(sys.stdin)
for a in d.get('artifacts', []):
    print(f\"{a['path']}: {a['bytes']} bytes\")
"
```

### Critério 4 — Telemetria real

```bash
curl -s http://10.15.10.3:8080/api/telemetry | python3 -m json.tool
# Esperado (shape top-level, sem items[]):
#   gpus[] = ["NVIDIA GeForce RTX 3060", "NVIDIA GeForce GTX 1660 SUPER"]
#   vramTotal > 0
#   vramUsed > 0
```

Dashboard (`http://10.15.10.3:3000`) deve mostrar gauges reais de GPU.

### Critério 5 — Falha honesta (yolo11x > 12GB)

Submeter um job com modelo que excede VRAM:
```bash
curl -b cookies.txt -X POST http://10.15.10.3:8080/api/jobs/yolo \
  -H 'Content-Type: application/json' \
  -d '{"model":"yolo11x","epochs":1,"batch":16,"imgsz":640}'
```

Verificar:
```sql
SELECT status, params->>'error' AS error FROM jobs ORDER BY created_at DESC LIMIT 1;
-- Esperado: status='failed', error contém mensagem de OOM ou CUDA out of memory
```

> O pipeline reporta o erro; **não é silencioso**.

### Critério 6 — Telemetria após falha

```bash
curl -s http://10.15.10.3:8080/api/telemetry | python3 -m json.tool
# jobsActive deve decrementar corretamente após o failed
```

---

## 6. Teardown (restaurar dev host)

### 6.1 Down do projeto GPU no TrueNAS

```bash
ssh dockeruser@10.15.1.2 \
  'cd /mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio && \
   docker compose -p gpu -f infra/compose.gpu.yaml down -v'
```

> `-v` remove volumes `gpu_datasets` e `gpu_outputs` (stateless — D1 ADR-0010).

### 6.2 Restaurar dev host

Edite `infra/.env` e **remova** ou comente:
```
# SEAWEED_PUBLISH=10.15.10.3    ← voltar ao default (127.0.0.1)
# TRAINER_IMAGE=hephaestus/trainer-yolo:gpu    ← voltar ao default (…:local)
# AUTO_ADOPT_LOCAL=0             ← não é mais necessário (fatia H), mas inofensivo se mantido
```

Recriar serviços:
```bash
docker compose -f infra/compose.yaml up -d seaweedfs --force-recreate
docker compose -f infra/compose.yaml up -d manager --force-recreate
docker compose -f infra/compose.yaml up -d orchestrator-local
```

### 6.3 Revogar orquestrador remoto via API

```bash
# Obter o ID do orquestrador remoto:
REMOTE_ID=$(curl -s http://10.15.10.3:8080/api/orchestrators \
  | python3 -c "import sys,json; [print(o['id']) for o in json.load(sys.stdin) if o['kind']=='remoto']")

# Revogar:
curl -X POST "http://10.15.10.3:8080/api/orchestrators/${REMOTE_ID}/revoke"
# Ou: botão "Revogar" na página /environments
```

### 6.4 Restaurar orquestrador local

```bash
docker compose -f infra/compose.yaml start orchestrator-local
```

O orquestrador local faz heartbeat e o manager re-adota
automaticamente (se `AUTO_ADOPT_LOCAL=1`, que é o default).

Para descobrir o pairing code do local:
```bash
docker logs infra-orchestrator-local-1 2>&1 | grep pairing
```

> Se precisar re-adotar manualmente, use o pairing code do log:
> `POST /api/orchestrators/adopt` com `kind: "local"` e o código.

### 6.5 Verificar restore

```bash
# Orquestrador local respondendo:
curl -s http://10.15.10.3:8082/health

# S3 em loopback:
curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:8333/
# Esperado: 403

# Banco com 1 row local:
curl -s http://10.15.10.3:8080/api/orchestrators \
  | python3 -c "import sys,json; [print(f\"{o['name']} kind={o['kind']} status={o['status']}\") for o in json.load(sys.stdin)]"
# Esperado: orchestrator-local kind=local status=online
```

---

## Notas

- **Envelope seguro de VRAM** (D9 ADR-0010):
  - 3060 (12GB): `yolo11n` batch=16, `yolo11m` batch=8
  - 1660S (6GB): `yolo11n` batch=8, imgsz=640
  - `yolo11x` / batch≥32 tende a OOM → job `failed` (falha honesta)
- **Heartbeat sem identidade** (ADR-0009 R1): o cache global de telemetria
  é sobrescrito a cada ~2s. Só 1 orquestrador deve estar online por vez
  durante a sessão.
- **Portas no TrueNAS**: o orquestrador GPU usa `8082` (a mesma do local,
  mas em host diferente — sem conflito de porta cross-host).
- **NÃO tocar nos 48 containers** do TrueNAS — o projeto `gpu` é isolado.
