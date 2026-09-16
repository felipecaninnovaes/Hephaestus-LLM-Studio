# Arquitetura do Sistema & Diretrizes dos Quatro Pilares

Padrões arquiteturais, separação de responsabilidades, isolamento de rede e convenções de código para o Hephaestus LLM Studio.

---

## 1. Estrutura dos Quatro Pilares

O repositório estrutura-se sob quatro pilares rígidos:

```text
.
├── apps/                   # 1. Aplicações cliente / frontend (UI voltada ao usuário final)
│   └── web/                # Next.js 16 (App Router), React 19, TypeScript, Tailwind CSS v4
├── services/               # 2. Serviços backend de aplicação & Gateways
│   ├── api-principal/      # Rust Axum (:8080) — Única porta pública da API (BFF, Auth, Datasets, S3)
│   ├── manager/            # Rust Axum (:8081) — Gerenciador de fila, VRAM e alocação de nós
│   └── orchestrator/       # Rust Axum — Executor stateless de engines via Docker/subprocess
├── engines/                # 3. Motores de IA e alta performance (ISOLAMENTO TOTAL de rede)
│   ├── trainer-clip/       # Python (uv) + OpenCLIP (Embeddings, busca semântica)
│   ├── trainer-difusao/    # Python (uv) + Diffusers/PyTorch (Flux, SDXL, SD 1.5, LoRA)
│   └── trainer-yolo/       # Python (uv) + Ultralytics (Detecção e tracking)
├── infra/                  # 4. Infraestrutura como Código, manifests Docker e scripts
│   ├── compose.yaml        # Blueprint Docker (Postgres/pgvector, SeaweedFS S3, services)
│   └── scripts/            # Scripts determinísticos de checagem e teste
├── packages/               # Contratos e políticas compartilhadas
│   ├── contracts/          # Schemas OpenAPI e DTOs tipados
│   └── policies/           # engines.yaml, vram-table.yaml
└── tasks/
    └── todo.md             # Memória persistente da sessão (Documentar e Limpar)
```

---

## 2. Responsabilidades & Boundaries dos Serviços

| Componente | Linguagem/Stack | Responsabilidade | Exposição de Rede |
| :--- | :--- | :--- | :--- |
| **`apps/web`** | Next.js 16 + TS + Tailwind v4 | Interface de usuário (Studio, Datasets, AutoLabel, AutoTracker, Playground). | Porta `:3000` no host (rede default do compose). |
| **`services/api-principal`** | Rust (Axum, SQLx, Tokio) | Ponto único de entrada da UI (BFF). Auth single-user, CRUD de datasets/imagens/labels, S3 storage abstraction. Comunica-se com `manager`. | **Única porta pública de API `:8080`**. |
| **`services/manager`** | Rust (Axum, SQLx, Tokio) | Gestão da fila de jobs, controle de VRAM (`packages/policies/vram-table.yaml`), heartbeat e alocação de nós. Escreve `jobs`/`job_artifacts`/`orchestrators` no schema compartilhado (migrations vivem em `services/api-principal/migrations/`). | `:8081` — uso interno, publicada no host apenas em dev; Bearer token. |
| **`services/orchestrator`** | Rust (Axum, Tokio) | Executor stateless. Recebe dispatch do manager e executa contêiner/subprocesso da engine correspondente. | `:8082` no nó de execução (dev publicada; em GPU/TrueNAS exposta só à rede do nó). |
| **`engines/*`** | Python 3.11+ (uv, PyTorch) | Treinamento e inferência real de modelos. Dev opera em modo CPU via `ENGINE_MOCK=1`. | **ISOLAMENTO TOTAL.** PROIBIDO mapear `ports:` para o host; embedder/daemon têm bind loopback ou ficam só na rede do compose. |

---

## 3. Isolamento Estrito de Rede das Engines

- **Regra de Segurança Inviolável:** Motores em `engines/*` **NUNCA** devem expor portas diretamente ao host da máquina ou à internet pública.
- Todo acesso externo passa obrigatoriamente por `apps/web` -> `services/api-principal` -> `services/manager` -> `services/orchestrator` -> `engines/*`.
- A comunicação entre serviços e engines ocorre na rede default do Docker Compose; o isolamento real das engines é a **ausência de mapeamento de portas** para o host (nunca redes customizadas — não existem no compose atual).

---

## 4. Ports & Adapters & Clean Architecture

- **Backend Rust:** O núcleo de domínio não possui acoplamento com bibliotecas HTTP ou clientes de banco/S3. O domínio opera através de traits abstratas (portas, ex: `StoragePort`), implementadas por adaptadores dedicados (`S3Storage`, `MockStorage`).
- **Banco de Dados (Postgres + pgvector):** schema único com migrations em
  `services/api-principal/migrations/` (0001..0013). Posse lógica por domínio:
  - `api-principal`: `users`/`auth_state`, `datasets`, `dataset_versions`,
    `images`, `videos`, `boxes`, `classes`, `captions`, `image_embeddings`,
    `models`, `generations`.
  - `manager`: `jobs` (incl. `phase`/`message`), `job_artifacts`, `orchestrators`.
- **Frontend Next.js:** Separação entre componentes visuais reutilizáveis (`components/ui`) e componentes de tela de domínio (`components/studio`). Acesso a dados centralizado em `lib/api.ts` apontando exclusivamente para `:8080`.

---

## 5. Disciplina de Comentários no Código

- **O que comentar (OBRIGATÓRIO):**
  - O **porquê** de decisões arquiteturais não óbvias e alternativas rejeitadas;
  - Invariantes de domínio e contratos implícitos entre serviços (ex: convenção camelCase no wire `/api/*`, snake_case no banco);
  - Trade-offs deliberados (ex: priorização de latência vs consumo de VRAM);
  - Premissas de concorrência, locks e tratamento de falhas em filas distribuídas.
- **O que NÃO comentar (PROIBIDO):**
  - Parafrasear código óbvio (ex: `// inicia o servidor na porta 8080`);
  - Repetir nomes de funções, structs ou métodos autoexplicativos;
  - Blocos de código morto comentados (devem ser apagados; o histórico do Git é a fonte da verdade).
