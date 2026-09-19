# Infraestrutura: Topologia, Perfis e Ingress

Visão geral do pilar de Infraestrutura do Hephaestus LLM Studio (`infra/`), definindo os perfis de implantação via Docker Compose, topologia de rede, regras de isolamento e proxy reverso.

---

## 1. Os Quatro Perfis de Implantação Compose

O ciclo de vida da infraestrutura é modularizado em overlays declarativos do Docker Compose:

| Arquivo | Papel / Ambiente | Serviços Subidos | Casos de Uso |
| :--- | :--- | :--- | :--- |
| **`infra/compose.yaml`** | **Desenvolvimento / Local-First** | `db`, `seaweedfs`, `s3-init`, `embedder`, `principal`, `manager`, `orchestrator-local`, `web` | Ambiente de desenvolvimento diário no dev host com mock ou Docker local. |
| **`infra/compose.prod.yaml`** | **Produção / Ingress Unificado** | Overlay sobre `compose.yaml`: adiciona `ingress` (Caddy) e fecha portas diretas do host | Deploy exposto com proxy reverso unificado na porta 80/443 e TLS. |
| **`infra/compose.gpu.yaml`** | **Nó GPU Dedicado / TrueNAS** | `orchestrator-gpu` (projeto `-p gpu`) | Worker remoto conectado via rede local (LAN) com aceleração NVIDIA CUDA. |
| **`infra/compose.integ.yaml`** | **Integração / CI** | Overlay sobre `compose.yaml`: `ENGINE_MOCK=1`, credenciais de teste | Suíte de testes de integração e pipeline automatizado (Gitea/CI). |

---

## 2. Topologia de Rede e Comunicação

Todos os serviços locais comunicam-se através de uma rede bridge Docker isolada (`infra_default`):

```
                                  [ Caddy (Ingress :80/:443) ]  (Overlay Prod)
                                                |
                               +----------------+----------------+
                               |                                 |
                               v (Proxy /api/*)                  v (Proxy /*)
                     [ api-principal :8080 ]               [ web :3000 ]
                               |
                   +-----------+-----------+
                   |                       |
                   v                       v
          [ manager :8081 ]       [ seaweedfs :8333 ] <----+ [ s3-init ] (Run-once)
                   |                       ^
                   v                       |
       [ orchestrator-local :8082 ] -------+
                   | (Docker Socket)
                   v
       [ trainer-yolo / difusao ] (Containers efêmeros sem porta)
```

### Regras de Conexão Entre Nós (LAN)
Quando o orquestrador executa em um nó GPU remoto (ex.: TrueNAS):
- **Dev Host (`10.15.10.3`):** hospeda `db` (Postgres), `manager` (:8081), `seaweedfs` (:8333) e `principal` (:8080).
- **Nó GPU (`10.15.1.2`):** hospeda `orchestrator-gpu` (:8082). Comunica-se diretamente via IP/porta de rede local com `manager:8081` e `seaweedfs:8333`.

---

## 3. Políticas Inegociáveis de Rede e Segurança

### 3.1 Isolamento Estrito das Engines
- **Zero ports no host:** É estritamente proibido expor qualquer porta das engines (`trainer-yolo`, `trainer-difusao`) diretamente no host.
- **Isolamento por arquitetura:** O browser e a LAN nunca se comunicam diretamente com as engines. A cadeia de comando obrigatória é:
  $$\text{Web (Browser)} \longrightarrow \text{api-principal} \longrightarrow \text{manager} \longrightarrow \text{orchestrator} \longrightarrow \text{engine}$$
- As engines operam em containers efêmeros provisionados pelo `orchestrator` via `/var/run/docker.sock` ou como daemon interno.

### 3.2 Binds Seguros de Loopback (`127.0.0.1`)
Para evitar exposição não intencional em redes locais no ambiente de desenvolvimento:
- `db` (PostgreSQL): `${DB_PUBLISH:-127.0.0.1}:5432:5432`
- `seaweedfs` (S3 API): `${SEAWEED_PUBLISH:-127.0.0.1}:8333:8333` (e master em `127.0.0.1:9333`)
- `manager`: `${MANAGER_PUBLISH:-127.0.0.1}:8081:8081`
- `orchestrator-local`: `${ORCHESTRATOR_PUBLISH:-127.0.0.1}:8082:8082`
- `embedder`: `127.0.0.1:8090:8090` (sem autenticação, loopback estrito)

Apenas `web` (:3000) e `api-principal` (:8080) são expostos abertamente em desenvolvimento.

---

## 4. Ingress e Proxy Reverso (`infra/Caddyfile`)

Em ambiente de produção (`compose.prod.yaml`), o serviço `ingress` utiliza o Caddy 2.8 como ponto único de entrada:

- **Roteamento de API:** `/api/*` e `/metrics` são encaminhados para `principal:8080`.
- **Roteamento de Frontend:** Todas as demais requisições vão para o container Next.js `web:3000`.
- **Encerramento de Portas Diretas:** O overlay sobrescreve `ports: !override []` para `web` e `principal`, impedindo bypass do proxy.
- **Headers de Segurança Injetados:**
  - `X-Content-Type-Options: nosniff`
  - `X-Frame-Options: DENY`
  - `Referrer-Policy: no-referrer-when-downgrade`
  - Compressão dinâmica zstd/gzip.

---

## 5. Operação e Comandos Canônicos

```bash
# Subir ambiente dev padrão (CPU / mock)
docker compose -f infra/compose.yaml up -d

# Subir com ingress Caddy de produção
docker compose -f infra/compose.yaml -f infra/compose.prod.yaml up -d

# Subir suíte de integração para testes
docker compose -f infra/compose.yaml -f infra/compose.integ.yaml up --abort-on-container-exit

# Reconstruir imagens locais de motores de treinamento (profiles de build)
docker compose -f infra/compose.yaml --profile build build trainer-yolo
docker compose -f infra/compose.yaml --profile build build trainer-difusao

# Verificar integridade e sintaxe da configuração
docker compose -f infra/compose.yaml config -q

# Desligamento com preservação de volumes
docker compose -f infra/compose.yaml down

# Desligamento completo com limpeza de volumes (recriação limpa)
docker compose -f infra/compose.yaml down -v
```
