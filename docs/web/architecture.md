# Arquitetura Web (`apps/web`)

O `apps/web` é o frontend unificado do Hephaestus LLM Studio. Ele provê uma interface moderna, reativa e densa para treinamento supervisionado, anotação assistida, catalogação de modelos e playground generativo de difusão.

---

## Stack Tecnológica

- **Framework**: Next.js 16 (App Router com Server Components e Client Components especializados).
- **Compilação**: Turbopack para Hot Module Replacement (HMR) instantâneo em desenvolvimento.
- **Estilização**: Tailwind CSS v4 (`@tailwindcss/postcss`), aproveitando CSS theme tokens modernos e paleta escura nativa para estações de trabalho de IA.
- **Tipagem**: TypeScript em modo estrito (`strict: true`), garantindo tipagem forte entre contratos de dados e interfaces de usuário.
- **Tooling**: Biome para linting e formatação de código com alta performance.

---

## Proxy de API e Comunicação com BFF

O frontend comunica-se exclusivamente com o BFF (`services/api-principal`, porta `:8080`):

1. **Rewrites Transparentes (`next.config.ts`)**:
   - Rotas sob `/api/:path*` e o endpoint `/health` são redirecionados internamente para a URL do `api-principal` (`API_INTERNAL_URL`, padrão `http://localhost:8080`).
   - Evita problemas de CORS e simplifica a gestão de cookies no navegador.
2. **Transferência de Grandes Volumes**:
   - `experimental.proxyClientMaxBodySize: "8200mb"`: Permite o upload direto e em blocos (*chunked*) de checkpoints e datasets pesados de até 8 GiB.
   - `experimental.proxyTimeout: 900_000`: Timeout expandido para 15 minutos para tolerar uploads volumosos e computação de MD5 em lote.
3. **Controle de Acesso e Sessão (`proxy.ts`)**:
   - Middleware leve que verifica a presença do cookie seguro `heph_session`.
   - Redireciona usuários não autenticados para `/login`, preservando o fluxo transparente para as chamadas `/api/*` (que recebem 401 direto do backend).

---

## Rotas e Páginas Principais

As páginas de estúdio compartilham o layout base `(studio)/layout.tsx`, que inclui a barra lateral (`Sidebar`) e a central de comandos rápidos (`ActionCenter`):

- **`/dashboard`**: Resumo operacional, volume de armazenamento canônico no bucket S3, contagem de datasets/modelos e saúde dos nós orchestrator.
- **`/datasets`**: Catálogo de bases de imagens, importação/exportação, inspeção de amostras e telas de anotação vetorial (`/datasets/[id]` e `/annotate/[imageId]`).
- **`/models`**: Gestão do catálogo de pesos base (SD 1.5, SDXL, FLUX.2, YOLO11), adapters LoRA, download remoto e upload manual.
- **`/treino`**: Configuração e disparo de treinamentos de visão computacional (YOLO11) e difusão generativa (LoRA/QLoRA).
- **`/geracao`**: Interface interativa de geração de imagens de difusão, integrando o painel de parâmetros (`GenerationPanel`) e o histórico visual (`GenerationGallery`).
- **`/jobs`**: Lista de execuções ativas e concluídas, cancelamento e visualização de telemetria.
- **`/environments`**: Topologia e status dos nós orchestrator registrados e alocação de dispositivos GPU.

---

## Organização de Componentes

- **`components/ui/`**: Primitivas desacopladas de domínio (Button, Modal, Drawer, Badge, SegmentedControl, ProgressBar).
- **`components/studio/`**: Componentes específicos das operações do Hephaestus (modais de configuração de treino, visualizadores de anotações, galeria comparativa).
- **`types/studio.ts`**: Interfaces e tipos de dados alinhados estritamente com os contratos OpenAPI (`packages/contracts/openapi.yaml`).
