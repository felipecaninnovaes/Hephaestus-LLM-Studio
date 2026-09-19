# Visão Geral da Arquitetura

O Hephaestus LLM Studio é uma plataforma local-first para curadoria de datasets, treinamento e inferência de modelos de visão computacional (YOLO, CLIP) e geração/difusão (FLUX, SDXL, SD 1.5).

## Os Quatro Pilares

O repositório é estruturado em quatro pilares arquiteturais bem delimitados:

1. **Apps (`apps/`):**
   - `apps/web`: Frontend em Next.js 16 (App Router), TypeScript e Tailwind CSS v4. Interface de usuário reativa com páginas de curadoria, anotações, visualização de jobs e telemetria.
2. **Services (`services/`):**
   - Microsserviços compilados em Rust com Axum e Tokio.
   - `api-principal` (:8080): Backend For Frontend (BFF) público e autenticação.
   - `manager` (:8081): Gerenciador de estado, persistência relacional e fila de jobs.
   - `orchestrator` (:8082): Agente local executor nos nós de computação GPU.
3. **Engines (`engines/`):**
   - Ambientes especializados em Python gerenciados com `uv`.
   - `trainer-yolo`: Treinamento e validação de modelos de detecção/segmentação YOLO.
   - `trainer-difusao`: Treinamento LoRA e inferência/geração de modelos de difusão.
   - `trainer-clip`: Extração de embeddings visuais para busca semântica em datasets.
4. **Infra (`infra/`):**
   - Configuração de orquestração Docker Compose (`infra/compose.yaml`).
   - PostgreSQL 16 com extensão `pgvector` para dados relacionais e vetoriais.
   - SeaweedFS fornecendo API compatível com S3 para armazenamento de artefatos e imagens.

## Fluxo Canônico Unidirecional

A comunicação operacional obedece a um fluxo linear e unidirecional de dependências:

```
[ Browser / UI ]
       │  (HTTP / SSE)
       ▼
[ api-principal (:8080) ]
       │  (HTTP interno)
       ▼
[ manager (:8081) ] ── (PostgreSQL / S3)
       │  (Dispatch HTTP)
       ▼
[ orchestrator (:8082) ]
       │  (Docker CLI / Socket)
       ▼
[ Engine Container (Python) ] ── (S3 Storage)
```

- **Passo 1 (Browser -> api-principal):** A aplicação web comunica-se exclusivamente com a `api-principal`, que valida credenciais, autorização e formatos de entrada.
- **Passo 2 (api-principal -> manager):** Requisições de execução ou consulta de jobs são delegadas ao `manager` com token interno.
- **Passo 3 (manager -> orchestrator):** O `manager` escalona tarefas na fila e faz o dispatch para o nó executor (`orchestrator`) que possui recursos de VRAM adequados.
- **Passo 4 (orchestrator -> Engine):** O `orchestrator` prepara o diretório de trabalho, baixa snapshots e sobe o container da Engine correspondente.

## Princípio de Isolamento de Rede das Engines

As engines de execução em Python operam sob isolamento rígido de rede:

- **Sem portas no host:** É expressamente proibido expor portas das engines para a máquina host no Compose (`ports:` proibido para engines em `infra/compose.yaml`).
- **Acesso restrito:** As engines comunicam-se apenas dentro da rede interna do Docker (`infra_default`) para alcançar o S3 ou responder via HTTP interno ao orchestrator.
- **Contenção de segurança:** Nenhuma ferramenta de inferência ou runtime de terceiros (como Gradio, FastAPI ou servidores de debug em Python) fica acessível fora do ambiente isolado.

## Contratos Canônicos

- **APIs e Schemas:** Especificações completas de rotas e payloads estão centralizadas em `packages/contracts/openapi.yaml`.
- **Dimensionamento de VRAM:** Regras e requisitos mínimos por arquitetura de modelo estão definidos em `packages/policies/vram-table.yaml`.
