# Arquitetura Web (`apps/web`)

O `apps/web` é o frontend unificado do Hephaestus LLM Studio. Ele provê uma interface moderna, reativa e densa para treinamento supervisionado, anotação assistida, catalogação de modelos e playground generativo de difusão.

---

## Stack Tecnológica

- **Framework**: Next.js 16 (App Router com Server Components e Client Islands especializados).
- **Compilação**: Turbopack para Hot Module Replacement (HMR) instantâneo em desenvolvimento.
- **Estilização**: Tailwind CSS v4 (`@tailwindcss/postcss`), aproveitando CSS theme tokens modernos (`@theme`) e paleta escura nativa (*Arcane Dark*) para estações de trabalho de IA.
- **Tipagem**: TypeScript em modo estrito (`strict: true`), garantindo tipagem forte entre contratos de dados OpenAPI e interfaces de usuário sem uso de `any`.
- **Tooling**: Biome para linting e formatação de código com alta performance.

---

## Proxy de API e Comunicação com BFF

O frontend comunica-se exclusivamente com o BFF (`services/api-principal`, porta `:8080`):

1. **Rewrites Transparentes (`next.config.ts`)**:
   - Rotas sob `/api/:path*` e o endpoint `/health` são redirecionados internamente para a URL do `api-principal` (`API_INTERNAL_URL`, padrão `http://localhost:8080`).
   - Evita problemas de CORS e simplifica a gestão de cookies no navegador.
   - Origens de desenvolvimento locais aceitas via `DEV_ALLOWED_ORIGIN` (padrão `127.0.0.1`).
2. **Transferência de Grandes Volumes**:
   - `experimental.proxyClientMaxBodySize: "8200mb"`: Permite o upload direto e em blocos (*chunked*) de checkpoints e datasets pesados de até 8 GiB.
   - `experimental.proxyTimeout: 900_000`: Timeout expandido para 15 minutos para tolerar uploads volumosos e computação de MD5 em lote.
3. **Controle de Acesso, Sessão e Interceptor 401 (`proxy.ts` e `lib/api.ts`)**:
   - Middleware leve que verifica a presença do cookie seguro `heph_session`.
   - Redireciona usuários não autenticados para `/login`, preservando o fluxo transparente para as chamadas `/api/*`.
   - Interceptor global em `lib/api.ts` que captura respostas 401 do backend e redireciona automaticamente para `/login?expired=1` preservando o caminho de retorno (`from=...`).

---

## Rotas e Páginas Principais

As páginas de estúdio compartilham o Server Component `(studio)/layout.tsx`, delegando o estado cliente para o invólucro fino `StudioShell.tsx` (Sidebar, ActionCenter, Breadcrumbs dinâmicos e ToastHost):

- **`/dashboard`**: Resumo operacional, volume de armazenamento no bucket S3, contagem de datasets/modelos e saúde dos nós orchestrator via `OrchestratorCard`.
- **`/datasets`**: Catálogo de bases de imagens, importação/exportação e filtros de curadoria.
- **`/datasets/[id]`**: Galeria de amostras com paginação infinita, filtros de divisão/classe/anotação, busca dinâmica (estrita ou semântica via embeddings CLIP), lixeira e envio em lote via `useDatasetGallery` e `useDatasetUpload`.
- **`/datasets/[id]/annotate/[imageId]`**: Editor de caixas delimitadoras YOLO com zoom vetorial, arrasto de canvas, atalhos de teclado (B, V, H, Del, 1-9), precisão sub-pixel e autosave com debounce via `useAnnotationCanvas` e `useAnnotationSync`.
- **`/models`**: Catálogo de pesos base (SD 1.5, SDXL, FLUX.2, YOLO11), adapters LoRA, upload multipart S3, renomeação e download.
- **`/treino`**: Forja dedicada ao treinamento de visão computacional (YOLO11: detecção e segmentação).
- **`/difusao`**: Forja de Difusão LoRA (`ForjaDifusaoSetup`), com estimativa em tempo real de VRAM, alertas preventivos de CUDA OOM, gerenciamento de presets JSON, bucketing por aspect ratio e suporte a checkpoints customizados FLUX.2/SDXL.
- **`/geracao`**: Interface interativa de geração de imagens de difusão (`GenerationPanel`), integrando painel de parâmetros, img2img, geração em lote e histórico visual (`GenerationGallery`).
- **`/playground`**: Workspace de inferência e detecção YOLO em tempo real sobre datasets com overlay vetorial de bounding boxes.
- **`/jobs`**: Lista de execuções ativas e concluídas, cancelamento, telemetria em tempo real, download de artefatos e visualização de curvas de convergência (`ConvergenceChart`).
- **`/environments`**: Topologia e status dos nós orchestrator registrados e alocação de dispositivos GPU (`OrchestratorCard`).

---

## Feedback e Boundary de Erros no App Router

- **`(studio)/loading.tsx`**: Skeleton de carregamento com estética Arcane Dark e cards em vidro óptico renderizado durante navegação lenta de rede.
- **`(studio)/error.tsx`**: Error Boundary no nível de rota para conter falhas inesperadas de renderização sem quebrar o estúdio, com digest de erro e botão de recuperação.
- **`(studio)/not-found.tsx`**: Página 404 integrada à identidade visual com ação de retorno ao painel principal.

---

## Organização de Camadas e Componentes

```
apps/web/
├── app/(studio)/           # Rotas do Studio (App Router Next.js 16)
│   ├── layout.tsx          # Server Component com metadata e isolamento de shell
│   ├── loading.tsx         # Skeleton loader visual Arcane UI
│   ├── error.tsx           # Error boundary client-side
│   ├── not-found.tsx       # Página 404 estilizada
│   └── */page.tsx          # Páginas orquestradoras compactas (<150–390 LOC)
├── components/
│   ├── ui/                 # Primitivas agnósticas de domínio (Button, Modal, Drawer, Table,
│   │                       # Checkbox, Switch, Tooltip, Alert, FormField, Kbd, ZoomControl)
│   ├── composite/          # Blocos compostos compartilhados (OrchestratorCard)
│   └── studio/             # Componentes de negócio modularizados por domínio:
│       ├── action-center/  # Centro de atividades lateral, notificações de nó e logs
│       ├── annotation/     # Sidebar de classes e canvas vetorial do editor YOLO
│       ├── charts/         # Curvas de convergência, sparklines e matemática vetorial
│       ├── dataset-detail/ # Header, toolbar, grade de amostras e modais de galeria
│       ├── diffusion/      # Configuração da Forja de Difusão, presets e estimador VRAM
│       ├── generation/     # Painel de geração, img2img, parâmetros e lightbox
│       ├── models/         # Cards de checkpoint, cabeçalho e renomeação
│       └── playground/     # Controles e overlay vetorial de predições YOLO
├── hooks/                  # Regras de negócio, ciclo de vida e estado fora da UI
│   ├── useAnnotationCanvas.ts   # Matemática de coordenadas, zoom/pan e listeners de teclado
│   ├── useAnnotationSync.ts     # Autosave debounced e reconciliação de IDs com backend
│   ├── useDatasetGallery.ts     # Paginação, busca, filtros e lixeira da galeria
│   ├── useDatasetUpload.ts      # Fila de upload em lote, progresso e auditoria
│   ├── useHardwareTelemetry.ts  # Polling de hardware e capacidade de VRAM do nó
│   ├── useJobLifecycle.ts       # Ações de abort, delete, apply boxes/captions e downloads
│   ├── useVramEstimator.ts      # Cálculo puro de risco de estouro de VRAM (CUDA OOM)
│   ├── useYoloPlayground.ts     # Polling e execução de inferência YOLO
│   ├── useFocusTrap.ts          # Confinamento acessível de foco para modais e gavetas
│   └── useBodyScrollLock.ts     # Trava de rolagem de página para diálogos
├── types/                  # Contratos particionados por domínio:
│   ├── common.ts, datasets.ts, models.ts, jobs.ts (com união tipada JobParams),
│   ├── diffusion.ts, yolo.ts, telemetry.ts, index.ts
│   └── studio.ts           # Barrel canônico de retrocompatibilidade
└── lib/                    # Clientes de API, formatação e persistência
    ├── api.ts              # apiFetch com interceptor global de 401
    ├── format.ts           # Formatadores canônicos unificados (formatBytes, formatDuration)
    └── geracao-storage.ts  # Persistência local e eventos cross-tab
```
