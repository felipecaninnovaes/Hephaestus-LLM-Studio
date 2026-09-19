# Infraestrutura: Topologia, Perfis e Ingress

Visão geral do pilar de Infraestrutura do Hephaestus LLM Studio (`infra/`), definindo os perfis de implantação via Docker Compose, topologia de rede, regras de isolamento, proxy reverso e diretrizes de hardening e performance para produção.

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

## 5. Hardening de Segurança e Diretrizes de Produção

### 5.1 Fail-Fast de Credenciais Padrão
Em desenvolvimento, variáveis como `STUDIO_PASSWORD`, `STUDIO_MASTER_KEY` e `MANAGER_TOKEN` possuem fallbacks relaxados (`changeme`, `studio`).
- **Regra de Produção:** O ambiente de produção **deve** validar que nenhuma credencial padrão está ativa. No boot do `compose.prod.yaml`, se qualquer variável contiver `changeme` ou `studio`, o serviço deve abortar a inicialização com código de erro 1.
- **Docker Secrets:** Recomenda-se a transição de variáveis de ambiente puras para Docker Secrets (`/run/secrets/*`) para evitar vazamento em saídas de `docker inspect`.

### 5.2 Execução com Usuários Não-Root
- Containers de aplicação (`web`, `api-principal`, `manager`) devem rodar sob usuários sem privilégios (`USER node` no frontend; `USER studio:1000` nos binários Rust).
- **Proteção do Docker Socket:** O `orchestrator` requer acesso ao socket Docker para subir engines. Em produção, deve-se utilizar um proxy de socket restrito (ex.: `docker-socket-proxy`) permitindo apenas verbos de criação de containers efêmeros e bloqueando acesso ao host, privilégios elevados (`privileged: true`) ou volumes do sistema operacional.

---

## 6. Gestão de Recursos e Orçamento de Memória

Para evitar starvation do host e erros fatais de Out-Of-Memory (OOM Killer):

| Serviço | Limite Recomendado de RAM | Limite de CPU | Justificativa |
| :--- | :--- | :--- | :--- |
| **`db` (Postgres + pgvector)** | 2.0 GB | 2.0 cores | Acomodar índices vetoriais HNSW e buffers de página. |
| **`seaweedfs`** | 1.5 GB | 1.5 cores | Operação de I/O e streaming de artefatos grandes. |
| **`principal` (BFF)** | 1.0 GB | 1.0 core | Uploads em streaming e processamento de requests HTTP. |
| **`manager`** | 512 MB | 0.5 core | Fila de jobs em memória e despachos assíncronos. |
| **`orchestrator-local`** | 1.0 GB | 1.0 core | Daemon e gerenciamento de processos sem a engine. |
| **`web` (Next.js)** | 1.0 GB | 1.0 core | Renderização SSR e roteamento de páginas do Studio. |

Esses limites devem ser declarados via bloco `deploy.resources.limits` no overlay de produção.

---

## 7. Padronização de Logs e Observabilidade

### 7.1 Rotação Uniforme de Logs
Todos os serviços do Compose devem aplicar a âncora declarativa de rotação de log para evitar esgotamento de disco:
```yaml
x-logging: &default-logging
  driver: "json-file"
  options:
    max-size: "10m"
    max-file: "3"
```
*Garantir que todos os serviços (`manager`, `orchestrator`, `embedder`, `db`, `principal`, `web`) herdem `logging: *default-logging`.*

### 7.2 Coleta de Métricas Prometheus
- O endpoint `/metrics` exposto pelo `api-principal` agrega telemetria dos serviços internos.
- No Caddy, a rota `/metrics` é protegida e exposta para scrapers externos (Prometheus / VictoriaMetrics).

---

## 8. Otimização de Imagens e Performance de Build

1. **Next.js Standalone Mode (`apps/web`):**
   - Habilitar `output: 'standalone'` em `next.config.ts`.
   - Copiar apenas `.next/standalone` e arquivos estáticos no Dockerfile, reduzindo a imagem de ~850 MB para ~150 MB.
2. **BuildKit Cache Mounts no Rust:**
   - Adicionar `--mount=type=cache,target=/usr/local/cargo/registry` e `--mount=type=cache,target=/app/target` nos Dockerfiles dos serviços Rust, acelerando rebuilds locais de minutos para segundos.

---

## 9. Operação e Comandos Canônicos

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
