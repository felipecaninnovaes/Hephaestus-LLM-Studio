# Infraestrutura: Armazenamento e Persistência

Guia canônico sobre as camadas de persistência, banco de dados vetorial/relacional, Object Storage S3 (SeaweedFS) e volumes gerenciados do Hephaestus LLM Studio.

---

## 1. Visão Geral das Camadas de Armazenamento

A persistência de dados no Hephaestus é segregada em três níveis de retenção e propósito:

| Camada | Tecnologia | Papel & Dados Armazenados | Volume / Caminho |
| :--- | :--- | :--- | :--- |
| **Relacional & Vetores** | PostgreSQL 16 + pgvector | Schemas de jobs, nós, usuários, tags e embeddings de busca semântica. | Volume `pgdata` |
| **Object Storage S3** | SeaweedFS (API S3) | Imagens de datasets, checkpoints de modelos (`.safetensors`), artefatos exportados. | Volume `seaweed_data` (Bucket: `heph-data`) |
| **Cache Local de Disco** | Volumes Docker montados | Staging de datasets de treino, cache de modelos HuggingFace e staging de saídas. | Volumes `datasets`, `models`, `outputs` |

---

## 2. Banco de Dados: PostgreSQL + pgvector

O banco de dados do sistema utiliza uma imagem oficial com a extensão `pgvector` fixada por digest:

- **Imagem:** `pgvector/pgvector:pg16-trixie@sha256:c8483555ce48101872f888c1df8a895ff689d6c7c7a5f7ac266475f9dfe89e0b`
- **Banco e Usuário Padrão:** `studio` / `studio` (sobrescrevível via `POSTGRES_PASSWORD`).
- **Segurança de Bind:** Limitado a `${DB_PUBLISH:-127.0.0.1}:5432:5432` no dev host.
- **Cold Boot & Healthcheck:**
  ```yaml
  healthcheck:
    test: ["CMD-SHELL", "pg_isready -U studio -d studio"]
    interval: 5s
    timeout: 3s
    retries: 10
  ```
  *Nota operacional:* A inicialização com volume limpo (`initdb`) requer tempo de inicialização antes de registrar no DNS interno. O `api-principal` depende da condição `service_healthy` do `db` para evitar crash loops. As migrações SQLx são aplicadas automaticamente no boot do `api-principal`.

---

## 3. Object Storage: SeaweedFS S3

O SeaweedFS provê um object storage compatível com a API AWS S3, operando em modo local-first de alta performance com baixo consumo de memória.

### 3.1 Identidades e Controle de Acesso (`infra/seaweedfs-s3.json`)
O SeaweedFS carrega identidades estritas via arquivo de configuração montado como `:ro`:

```json
{
  "identities": [
    {
      "name": "heph-admin",
      "credentials": [{ "accessKey": "heph", "secretKey": "heph-local-dev" }],
      "actions": ["Admin", "Read", "Write", "List", "Tagging", "UserManagement"]
    },
    {
      "name": "heph-orchestrator",
      "credentials": [{ "accessKey": "heph-orch", "secretKey": "heph-orch-local-dev" }],
      "actions": [
        "Read:heph-data/packages/*",
        "List:heph-data/packages/*",
        "Read:heph-data/artifacts/*",
        "Write:heph-data/artifacts/*",
        "List:heph-data/artifacts/*",
        "Read:heph-data/models/*",
        "List:heph-data/models/*"
      ]
    }
  ]
}
```
- **Princípio do Menor Privilégio:** O `orchestrator` recebe credenciais com escopo granular (`heph-orchestrator`), restritas aos prefixos necessários dentro do bucket `heph-data`.

### 3.2 Bootstrap com `s3-init` e `ensure-bucket.sh`
- **Problema Arquitetural:** Ao executar `docker compose down -v`, o volume `seaweed_data` é destruído. No próximo `up`, o SeaweedFS reinicia com as identidades declaradas, mas **sem o bucket `heph-data` criado**. Como a credencial do orchestrator não possui permissão `Admin` para auto-criação de buckets no primeiro PUT, uploads de artefatos de treino falhariam silenciosamente.
- **Solução (`s3-init`):** Um container efêmero (`alpine:3.20`) roda uma única vez no boot (`restart: "no"`) executando `infra/scripts/ensure-bucket.sh`.
- **Implementação do Script:**
  - Escrito em POSIX `sh` (compatível com BusyBox sem precisar de Python ou AWS CLI).
  - Assina o request `PUT /heph-data/` calculando manualmente o cabeçalho **AWS Signature Version 4 (SigV4)** via `openssl` e despacha via `curl`.
  - É estritamente **idempotente**: aceita tanto HTTP 200 quanto `BucketAlreadyOwnedByYou` ou `BucketAlreadyExists` como sucesso.

### 3.3 Endpoints e Presigned URLs (Gotcha de Assinatura SigV4)
Para entrega de imagens e download direto no browser:
- `S3_ENDPOINT_URL`: URL acessível internamente pelos containers (`http://seaweedfs:8333`).
- `S3_PUBLIC_ENDPOINT_URL`: URL alcançável pelo navegador do usuário (`http://localhost:8333` em dev ou `http://<ip-dev-host>:8333` em rede local).
- **Atenção:** As URLs pré-assinadas utilizam o hostname configurado em `S3_PUBLIC_ENDPOINT_URL` para o cálculo da assinatura SigV4. Se o operador acessar a interface web de outro IP da rede sem reconfigurar essa variável, o download falhará com erro de assinatura.

---

## 4. Volumes de Trabalho e Cache de Modelos

Os serviços de treinamento montam volumes nomeados gerenciados pelo Docker:

```yaml
volumes:
  pgdata:        # Dados relacionais do PostgreSQL
  seaweed_data:  # Objetos do SeaweedFS
  datasets:      # Staging de dados de treinamento (montado em /data/datasets)
  models:        # Cache local de modelos HF/Diffusers (montado em /data/models)
  outputs:       # Artefatos gerados pelo orchestrator (montado em /data/outputs)
```

### Staging com Hash MD5
Ao descarregar ou resolver pesos de modelos (ex.: FLUX.2 Klein, SD 1.5, CLIP), o `orchestrator` realiza verificação de integridade via checksum MD5/SHA256 e armazena os artefatos no volume compartilhado `models`, evitando downloads redundantes a cada novo job.
