# Infraestrutura: Topologia, Perfis e Ingress

Visão geral do pilar de Infraestrutura do Hephaestus LLM Studio (`infra/`), definindo os perfis de implantação via Docker Compose, topologia de rede, regras de isolamento, proxy reverso e diretrizes de hardening e performance para produção.

---

## 1. Os Quatro Perfis de Implantação Compose

O ciclo de vida da infraestrutura é modularizado em overlays declarativos do Docker Compose:

| Arquivo | Papel / Ambiente | Serviços Subidos | Casos de Uso |
| :--- | :--- | :--- | :--- |
| **`infra/compose.yaml`** | **Desenvolvimento / Local-First** | `db`, `seaweedfs`, `s3-init`, `embedder`, `principal`, `manager`, `orchestrator-local`, `web` | Ambiente de desenvolvimento no dev host com mock ou Docker local. |
| **`infra/compose.prod.yaml`** | **Produção / Ingress Unificado** | Overlay sobre `compose.yaml`: adiciona `ingress` (Caddy) e fecha portas diretas do host | Deploy exposto com proxy reverso unificado na porta 80/443 e TLS. |
| **`infra/compose.gpu.yaml`** | **Nó GPU Dedicado / TrueNAS** | `orchestrator-gpu` (projeto `-p gpu`) | Worker remoto conectado via rede local (LAN) com aceleração NVIDIA CUDA. |
| **`infra/compose.integ.yaml`** | **Integração / CI** | Overlay sobre `compose.yaml`: `ENGINE_MOCK=1`, credenciais de teste | Suíte de testes de integração e pipeline automatizado (Gitea/CI). |

---

## 2. Topologia de Rede e Segmentação em Zonas

A infraestrutura abandona o modelo de rede plana (*flat network*) e adota isolamento estrito segmentado em 3 redes bridge Docker (`frontend_net`, `backend_net` e `engine_net`):

1. **`frontend_net` (`${COMPOSE_PROJECT_NAME:-infra}_frontend_net`):**
   - **Integrantes:** `ingress` (Caddy, em prod), `web` (Next.js) e `principal` (BFF Axum).
   - **Finalidade:** Tráfego de borda e SSR. O container `web` tem isolamento estrito: **não tem acesso direto ao banco de dados (`db`) nem ao S3 (`seaweedfs`)**. Qualquer requisição para dados passa pelo proxy `/api/*` encaminhado ao `principal`.
2. **`backend_net` (`${COMPOSE_PROJECT_NAME:-infra}_backend_net`):**
   - **Integrantes:** `principal`, `manager`, `db` (Postgres + pgvector), `seaweedfs`, `s3-init`, `embedder` (CLIP) e `orchestrator-local`.
   - **Finalidade:** Comunicação e persistência interna de dados, orquestração e gerenciamento de jobs.
3. **`engine_net` (`${COMPOSE_PROJECT_NAME:-infra}_engine_net`):**
   - **Integrantes:** `orchestrator-local`, `seaweedfs` e containers de treinamento/inferência das engines (`trainer-yolo`, `trainer-difusao`, `diffusion-daemon`).
   - **Finalidade:** Isolamento de execução de workloads de IA. `seaweedfs` e `orchestrator-local` atuam como ponte segura (*dual-homed*), permitindo que os containers de treino façam download/upload de artefatos no S3 sem acesso ao banco de dados ou aos serviços de frontend.

```
  [ Borda / Ingress ]
          |
          v
   ( frontend_net ) --------------------------------------------+
          |                                                     |
          v                                                     v
    [ web :3000 ]                                     [ api-principal :8080 ]
  (Next.js / SSR)                                       (BFF / Gateway)
                                                                |
   ( backend_net ) <--------------------------------------------+
          |
          +-------------------+--------------------+--------------------+
          |                   |                    |                    |
          v                   v                    v                    v
     [ db :5432 ]      [ manager :8081 ]    [ embedder :8083 ]   [ seaweedfs :8333 ] <---+ [ s3-init ]
   (Postgres/pgvector)   (Job Manager)        (CLIP Embeddings)    (S3 Storage)
                              |                                         ^
                              v                                         |
                 [ orchestrator-local :8082 ] --------------------------+
                              |
   ( engine_net ) <-----------+ (Docker Socket /var/run/docker.sock)
          |
          v
   [ trainer-yolo / difusao / diffusion-daemon ]
   (Containers efêmeros sem porta exposta)
```

---

## 3. Conectividade de Nós Remotos e Exposição na LAN

### 3.1 O Problema do Bind Padrão (`127.0.0.1`)
Por padrão, `manager:8081` e `seaweedfs:8333` possuem bind restrito em `127.0.0.1` (`${MANAGER_PUBLISH:-127.0.0.1}`, `${SEAWEED_PUBLISH:-127.0.0.1}`). Para que um nó GPU remoto (ex.: TrueNAS em `10.15.1.2`) consiga se conectar, o operador no dev host (`10.15.10.3`) precisa sobrescrever essas variáveis no arquivo `infra/.env`:

```bash
# infra/.env no dev host (10.15.10.3)
MANAGER_PUBLISH=0.0.0.0       # ou o IP exato da interface LAN: 10.15.10.3
SEAWEED_PUBLISH=0.0.0.0       # ou 10.15.10.3
S3_PUBLIC_ENDPOINT_URL=http://10.15.10.3:8333
```

### 3.2 Vetores de Risco na LAN e Mitigações
Ao abrir esses binds, dois serviços internos ficam expostos diretamente na rede local:
1. **Manager (:8081):** Protegido apenas pelo `MANAGER_TOKEN` compartilhado via header `Authorization: Bearer <token>`. O tráfego de rede circula em HTTP plaintext na LAN.
2. **SeaweedFS (:8333):** Exposto em HTTP puro sem TLS. Embora protegido por credenciais S3 (SigV4), qualquer host da LAN pode tentar negociar requisições contra a porta S3.

**Mitigações Obrigatórias em Ambientes Não-Confiáveis:**
- **Firewall no Dev Host (UFW / iptables):** Restringir as portas `8081` e `8333` estritamente ao IP do nó worker (`10.15.1.2`), bloqueando qualquer outro tráfego da LAN:
  ```bash
  sudo ufw allow from 10.15.1.2 to any port 8081 proto tcp
  sudo ufw allow from 10.15.1.2 to any port 8333 proto tcp
  ```
- **VPN Ponto-a-Ponto (WireGuard / Tailscale):** Encapsular o tráfego entre dev host e nó remoto em um túnel criptografado privado, mantendo os binds externos fechados na interface física.

---

## 4. Políticas Inegociáveis de Isolamento das Engines

- **Zero ports no host:** É proibido mapear portas de containers de treinamento (`trainer-yolo`, `trainer-difusao`) para o host.
- **Isolamento por arquitetura:** O browser e a rede nunca chamam diretamente as engines. A cadeia de comando obrigatória é:
  $$\text{Web (Browser)} \longrightarrow \text{api-principal} \longrightarrow \text{manager} \longrightarrow \text{orchestrator} \longrightarrow \text{engine}$$
- As engines operam em containers efêmeros provisionados pelo `orchestrator` via `/var/run/docker.sock` ou como daemon interno.

---

## 5. Ingress e Proxy Reverso (`infra/Caddyfile`)

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

## 6. Hardening de Segurança e Diretrizes de Produção

### 6.1 Fail-Fast Completo de Credenciais em Produção
Em desenvolvimento, variáveis possuem valores padrão inseguros (`changeme`, `studio`, `heph-local-dev`). No ambiente de produção, **todas** as credenciais abaixo devem ser validadas no boot e o serviço deve abortar caso encontre os defaults:

1. `STUDIO_PASSWORD` (default: `changeme`)
2. `STUDIO_MASTER_KEY` (default: `changeme`)
3. `MANAGER_TOKEN` (default: `changeme`)
4. `POSTGRES_PASSWORD` (default: `studio`)
5. `S3_ACCESS_KEY` e `S3_SECRET_KEY` (default: `heph` / `heph-local-dev`)
6. `S3_ORCH_ACCESS_KEY` e `S3_ORCH_SECRET_KEY` (default: `heph-orch` / `heph-orch-local-dev`)

> ⚠️ **Atenção ao `infra/seaweedfs-s3.json`:** As credenciais S3 estão declaradas estaticamente no arquivo `seaweedfs-s3.json` versionado no Git. Em produção, este arquivo **deve ser substituído** por um arquivo fora de versão com chaves criptográficas geradas de forma aleatória e montado via volume seguro ou Docker Secret.

### 6.2 Análise Real da Segurança do Docker Socket (`/var/run/docker.sock`)
O `orchestrator` executa o binário CLI `docker` diretamente (`tokio::process::Command::new("docker")`), invocando comandos como `docker run`, `docker stop`, `docker rm` e `docker ps`.

*Nota crítica sobre `docker-socket-proxy`:*
- Proxies como `docker-socket-proxy` filtram chamadas na API Docker apenas por **método HTTP e rota** (ex.: liberar `POST /containers/create=1`).
- Eles **não inspecionam o corpo JSON da requisição**. Consequentemente, se a criação de containers estiver liberada, o proxy **não impede** parâmetros maliciosos no corpo da requisição (como montagem de volumes do host `HostConfig.Binds: ["/:/host"]` ou `HostConfig.Privileged: true`).
- **Mitigações reais para isolamento de nós:**
  1. Em nós dedicados, operar o worker em uma máquina virtual isolada cujo host possa ser destruído sem impacto operacional.
  2. Avaliar o uso de runtimes seguros e sandboxed (ex.: `runsc`/gVisor ou Kata Containers) como runtime padrão do Docker no nó worker.
  3. Utilizar o modo `EXEC_MODE=subprocess` em ambientes onde o socket Docker não puder ser exposto (ex.: containers RunPod).

### 6.3 Execução com Usuários Não-Root
- Containers de aplicação (`web`, `api-principal`, `manager`) devem rodar sob usuários sem privilégios (`USER node` no frontend; `USER studio:1000` nos binários Rust).

---

## 7. Gestão de Recursos e Orçamento de Memória

Limites recomendados via `deploy.resources.limits` no overlay de produção para evitar starvation:

| Serviço | Limite Recomendado de RAM | Limite de CPU | Justificativa |
| :--- | :--- | :--- | :--- |
| **`db` (Postgres + pgvector)** | 2.0 GB | 2.0 cores | Acomodar índices vetoriais HNSW e buffers de página. |
| **`seaweedfs`** | 1.5 GB | 1.5 cores | Operação de I/O e streaming de artefatos grandes. |
| **`principal` (BFF)** | 1.0 GB | 1.0 core | Uploads em streaming e processamento de requests HTTP. |
| **`manager`** | 512 MB | 0.5 core | Fila de jobs em memória e despachos assíncronos. |
| **`orchestrator-local`** | 1.0 GB | 1.0 core | Daemon e gerenciamento de processos sem a engine. |
| **`web` (Next.js)** | 1.0 GB | 1.0 core | Renderização SSR e roteamento de páginas do Studio. |

---

## 8. Padronização de Logs e Observabilidade

### 8.1 Rotação Uniforme de Logs
Todos os serviços do Compose devem aplicar a âncora declarativa de rotação de log:
```yaml
x-logging: &default-logging
  driver: "json-file"
  options:
    max-size: "10m"
    max-file: "3"
```
*Garantir que todos os serviços (`manager`, `orchestrator`, `embedder`, `db`, `principal`, `web`) herdem `logging: *default-logging`.*

### 8.2 Coleta de Métricas Prometheus
- O endpoint `/metrics` exposto pelo `api-principal` agrega telemetria dos serviços internos.
- No Caddy, a rota `/metrics` é protegida e exposta para scrapers externos (Prometheus / VictoriaMetrics).

---

## 9. Otimização de Imagens e Performance de Build

1. **Next.js Standalone Mode (`apps/web`):**
   - Habilitar `output: 'standalone'` em `next.config.ts`.
   - Copiar apenas `.next/standalone` e arquivos estáticos no Dockerfile, reduzindo a imagem de ~850 MB para ~150 MB.
2. **BuildKit Cache Mounts no Rust:**
   - Adicionar `--mount=type=cache,target=/usr/local/cargo/registry` e `--mount=type=cache,target=/app/target` nos Dockerfiles dos serviços Rust, acelerando rebuilds locais de minutos para segundos.

---

## 10. Operação e Comandos Canônicos

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

# Desligamento rotineiro seguro (preserva volumes pgdata e seaweed_data)
docker compose -f infra/compose.yaml down
```

> 🛑 **PERIGO DE PERDA TOTAL DE DADOS:** O comando `docker compose down -v` apaga irreversivelmente os volumes de dados (`pgdata`, `seaweed_data`). Ele **NUNCA** deve ser executado como rotina operacional, apenas em procedimentos de purge/wipe de fábrica intencionais com backup prévio verificado.
