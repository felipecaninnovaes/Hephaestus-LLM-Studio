# Auditoria Técnica e Plano de Modularização: `apps/web`
**Projeto:** Hephaestus LLM Studio  
**Data:** 18 de Setembro de 2026  
**Escopo:** Frontend unificado (`apps/web` — Next.js 16.3.4, React 19.2.8, Tailwind v4.3.3, Biome 2.5.14, TypeScript strict)  
**Status:** Concluído (Auditoria Estritamente Somente Leitura)

---

## 1. Resumo Executivo & Métricas

Uma auditoria exaustiva e somente leitura foi executada sobre os 107 arquivos TypeScript/TSX do frontend `apps/web` (32.603 linhas de código). A aplicação possui excelente estética visual fundamentada na direção de arte *"The Arcane Foundry"* (`docs/DESIGN.md`), ausência de dependências circulares (verificado via `madge`) e conformidade com `tsc --noEmit` (0 erros de tipagem estrita).

No entanto, a arquitetura interna apresenta **grave fragmentação estrutural**, **monólitos colossais** (sete arquivos com mais de 1.000 linhas, atingindo até 1.846 linhas em um único componente) e uma **subversão do paradigma do Next.js App Router**: a aplicação opera como uma SPA clássica inteiramente renderizada no navegador.

### Principais Indicadores Quantitativos

| Métrica | Valor Encontrado | Diagnóstico & Benchmark Recomendado |
|---|:---:|---|
| **Total de Arquivos TS/TSX** | **107** | 40 em `components/studio`, 24 em `components/ui`, 21 em `lib`, 1 em `types`, 1 em `hooks`, 14 em `app` |
| **Linhas Totais de Código (TS/TSX)** | **32.603 LOC** | Média de 304 linhas/arquivo |
| **Arquivos com `"use client"`** | **68 (64,1%)** | **12 de 14 arquivos sob `app/`** são Client Components (7.843 LOC). Zero Server Components em rotas |
| **Top 10 Maiores Arquivos** | **13.627 LOC (41,8%)** | Apenas 10 arquivos concentram mais de 41% de todo o código da aplicação |
| **Clones de Código (jscpd)** | **65 clones detectados** | **1.083 linhas duplicadas (3,10% - 3,56% em TSX)**, concentradas entre jobs, telemetria e forjas |
| **Arquivos de Rotas Especiais** | **0 loading, 0 error, 0 not-found** | Inexistência de Streaming SSR e Error Boundaries nativos de rota |
| **Violações A11y (Modal/Drawer)** | **Zero Focus Trap / Focus Restore** | Modais e Drawers vazam foco via `Tab` e não travam scroll de forma segura |
| **Cores Fora dos Tokens** | **67 hex soltos / 38 arqs com rose/sky/indigo** | Violação da *Brand-Only Rule* do Design System Arcane v2 |
| **Código Morto & Duplicado** | **2 arquivos órfãos / 2 shims** | `CleanupJobsModal.tsx` (órfão), `ImportDatasetModal.tsx` (obsoleto), `Toast.tsx` e `ConfirmDialog.tsx` (shims) |

### Top 30 Maiores Arquivos por Linhas de Código

```
 1. 1846  apps/web/components/studio/GenerationPanel.tsx
 2. 1837  apps/web/components/studio/ForjaDifusaoSetup.tsx
 3. 1786  apps/web/app/(studio)/datasets/[id]/page.tsx
 4. 1718  apps/web/components/studio/ActionCenter.tsx
 5. 1598  apps/web/app/(studio)/jobs/page.tsx
 6. 1017  apps/web/components/studio/GenerationGallery.tsx
 7. 1005  apps/web/app/(studio)/playground/page.tsx
 8.  928  apps/web/components/studio/AutoLabelModal.tsx
 9.  887  apps/web/components/studio/Sidebar.tsx
10.  829  apps/web/types/studio.ts
11.  806  apps/web/app/(studio)/datasets/[id]/annotate/[imageId]/page.tsx
12.  755  apps/web/components/studio/CreateDatasetModal.tsx
13.  690  apps/web/components/studio/ConvergenceChart.tsx
14.  643  apps/web/components/ui/Select.tsx
15.  609  apps/web/lib/geracao-storage.ts
16.  588  apps/web/components/studio/ForjaYoloSetup.tsx
17.  542  apps/web/app/(studio)/environments/page.tsx
18.  538  apps/web/components/studio/ImageQuickLookModal.tsx
19.  534  apps/web/components/studio/AutolabelReviewModal.tsx
20.  533  apps/web/components/icons.tsx
21.  465  apps/web/components/studio/ModelUploadModal.tsx
22.  462  apps/web/components/studio/JobLogViewer.tsx
23.  444  apps/web/app/(studio)/dashboard/page.tsx
24.  430  apps/web/lib/dataset-inspector.ts
25.  430  apps/web/components/studio/DatasetCard.tsx
26.  430  apps/web/app/(studio)/datasets/page.tsx
27.  423  apps/web/app/(studio)/models/page.tsx
28.  398  apps/web/components/studio/BatchEditClassesModal.tsx
29.  349  apps/web/components/studio/AutotrackerReviewModal.tsx
30.  305  apps/web/components/studio/ClassesModal.tsx
```

---

## 2. Divergências entre `docs/web/architecture.md` e o Código Real

O documento `docs/web/architecture.md` foi tratado como hipótese conforme as regras da auditoria. A investigação confrontou cada afirmação documental contra o código implementado:

| Ponto Documentado em `architecture.md` | O Que Está Implementado no Código Real | Evidência no Código | Impacto / Gravidade |
|---|---|---|:---:|
| **Rotas:** `/treino` documentado como rota única de visão (YOLO11) e difusão (LoRA/QLoRA). | `/treino` é **exclusivo para YOLO**. A difusão foi bifurcada para a rota `/difusao`, que **nem sequer é citada** no documento. | `app/(studio)/treino/page.tsx:31` (`Treino YOLO`)<br>`app/(studio)/difusao/page.tsx:1-139` (`ForjaDifusaoSetup`) | **Alta** (desalinhamento funcional e de navegação) |
| **Rotas:** `/playground` não existe na documentação (apenas cita "playground generativo de difusão"). | `/playground` existe como página monólito de 1005 linhas dedicada à **inferência de detecção YOLO** (com rótulo "Detecção YOLO" na Sidebar). | `app/(studio)/playground/page.tsx:1-1005`<br>`components/studio/Sidebar.tsx:250` | **Alta** (rota órfã da arquitetura documental) |
| **Rotas:** Página `/login` ausente da lista de páginas principais (citada apenas no middleware). | `/login` existe como rota completa com formulário próprio e 190 linhas. | `app/login/page.tsx:1-190` | **Baixa** (omissão documental) |
| **Componentes UI:** Lista apenas `Button, Modal, Drawer, Badge, SegmentedControl, ProgressBar`. | Existem **24 componentes** em `components/ui`, incluindo `Select` (643 linhas), `Toast`, `GlassCard`, `ZoomControl`, etc. | `components/ui/*` (24 arquivos) | **Média** (catálogo subdocumentado) |
| **Camada de Dados & Hooks:** Afirmação genérica de "rewrites transparentes" sem descrever consumo de API. | Existe um cliente HTTP central (`lib/api.ts` com `ApiError`), mas **não há documentação** de polling de jobs, upload chunked, `lib/events.ts` ou gerenciamento de estado. | `lib/api.ts:1-57`<br>`lib/events.ts:1-15` | **Alta** (ponto cego arquitetural para novos desenvolvedores) |
| **Hooks:** Implica separação entre regras e UI. | Existe **apenas 1 hook** em toda a pasta `hooks/` (`useJobTelemetry.ts`). Quase 100% da lógica de negócio está inlined dentro dos componentes. | `apps/web/hooks/` (1 único arquivo) | **Crítica** (acoplamento extremo entre UI e infraestrutura) |
| **Upload Chunked:** Menciona apenas `proxyClientMaxBodySize: "8200mb"`. | Upload em partes com hashing MD5, paralelismo e retries foi implementado no client, mas inlined dentro de `ModelUploadModal.tsx:48-220`. | `components/studio/ModelUploadModal.tsx:48-220` | **Média** (falta de abstração compartilhada) |

---

## 3. Padrão-Alvo Proposto (Arquitetura Autônoma e Modular)

O padrão-alvo foi concebido para atender à regra de **Independência e Autonomia**: *qualquer desenvolvedor ou subagente deve conseguir criar ou alterar uma tela compondo primitivas e blocos sem duplicar código, sem violar camadas e mantendo os arquivos abaixo de 250 linhas*.

```
apps/web/
├── app/                              # NEXT.JS 16 APP ROUTER
│   ├── (auth)/login/page.tsx         # Rotas públicas
│   ├── (studio)/                     # Layout shell do Studio
│   │   ├── layout.tsx                # Server Component (sem "use client")
│   │   ├── loading.tsx               # Skeleton global da aplicação
│   │   ├── error.tsx                 # Error boundary global com reporte
│   │   ├── not-found.tsx             # 404 integrado ao layout do estúdio
│   │   ├── dashboard/page.tsx        # Server Component com fetch inicial + Client Islands
│   │   ├── datasets/                 # Rotas de Datasets (Server + Client Islands)
│   │   │   ├── page.tsx
│   │   │   ├── loading.tsx
│   │   │   ├── [id]/page.tsx
│   │   │   └── [id]/annotate/[imageId]/page.tsx
│   │   ├── models/page.tsx
│   │   ├── treino/page.tsx           # YOLO training
│   │   ├── difusao/page.tsx          # Diffusion LoRA training
│   │   ├── geracao/page.tsx          # Interactive generation & gallery
│   │   ├── playground/page.tsx       # YOLO interactive inference
│   │   ├── jobs/page.tsx
│   │   └── environments/page.tsx
│   ├── globals.css                   # Design Tokens (@theme Arcane v2)
│   └── layout.tsx                    # Root Layout Server Component
│
├── components/
│   ├── ui/                           # PRIMITIVAS ATÔMICAS (Zero Domain Knowledge)
│   │   ├── Button.tsx                # Named exports estritos, sem export default
│   │   ├── Modal.tsx                 # Com useFocusTrap e useBodyScrollLock
│   │   ├── Drawer.tsx
│   │   ├── ConfirmDialog.tsx         # Com role="alertdialog"
│   │   ├── Table.tsx                 # [NOVA] Primitiva canônica de tabelas
│   │   ├── Checkbox.tsx              # [NOVA] Primitiva canônica de seleção
│   │   ├── Switch.tsx                # [NOVA] Toggle acessível role="switch"
│   │   ├── Tooltip.tsx               # [NOVA] Tooltip visual Arcane acessível
│   │   ├── Popover.tsx               # [NOVA] Dropdown/Portal desacoplado
│   │   ├── Alert.tsx                 # [NOVA] Banners de erro/aviso role="alert"
│   │   ├── FormField.tsx             # [NOVA] Label mono + Hint + Error unificados
│   │   └── Select.tsx                # Refatorado (<180 linhas) consumindo hooks
│   │
│   ├── composite/                    # [NOVA] COMPOSTOS COMPARTILHADOS ENTRE DOMÍNIOS
│   │   ├── OrchestratorCard.tsx      # Card unificado de nó (usado em dashboard e envs)
│   │   ├── JobMetricChips.tsx        # Chips e tags de status de jobs
│   │   ├── FileUploader.tsx          # Dropzone compartilhado com validação
│   │   ├── EmptyState.tsx            # Padronizado
│   │   └── MetricGrid.tsx            # KPIs padronizados
│   │
│   └── studio/                       # COMPONENTES DE DOMÍNIO (Organizados por Feature)
│       ├── datasets/                 # Gallery, Inspection, BoundingBoxCanvas, Tools
│       ├── training/                 # ForjaYolo, ForjaDifusao decompostas (<250 LOC)
│       ├── generation/               # GenerationPanel e Gallery decompostos
│       ├── jobs/                     # JobCard, JobLogViewer, JobCleanupDialog
│       ├── environments/             # NodeSelect, AdoptionModal
│       └── shell/                    # Sidebar decomposta, ActionCenter decomposto
│
├── hooks/                            # HOOKS DE DOMÍNIO E INFRAESTRUTURA
│   ├── useJobTelemetry.ts            # SSE / Telemetria em tempo real
│   ├── useJobLifecycle.ts            # [NOVO] Mutadores unificados (abort, delete, rerun)
│   ├── useHardwareTelemetry.ts       # [NOVO] Polling consciente de GPU/VRAM
│   ├── useVramEstimator.ts           # [NOVO] Heurística preditiva de OOM unificada
│   ├── useFocusTrap.ts               # [NOVO] A11y para modais e gavetas
│   ├── useFloatingPosition.ts        # [NOVO] Matemática de flip/offset para Select/Tooltip
│   ├── useChunkedUpload.ts           # [NOVO] Upload multipart resiliente com MD5
│   └── useAnnotationCanvas.ts        # [NOVO] Motor vetorial e ponteiros do canvas YOLO
│
├── lib/                              # CLIENTES DE API & HELPERS PUROS
│   ├── api.ts                        # Fetch cliente com interceptor 401 central
│   ├── format.ts                     # Formatadores únicos (bytes, datas, tempos)
│   └── ...
│
└── types/                            # TIPOS DE DOMÍNIO ALINHADOS AO OPENAPI
    ├── index.ts                      # Barrel re-exportando tudo para retrocompatibilidade
    ├── common.ts                     # UUID, Pagination, Status
    ├── datasets.ts                   # Dataset, Image, BoundingBox, Splits
    ├── jobs.ts                       # Job, JobKind, JobParams (Tagged Union)
    ├── diffusion.ts                  # Presets, Hiperparâmetros, Amostras
    ├── yolo.ts                       # YOLO Hyperparameters, Prediction
    └── orchestrators.ts              # Orchestrator, HardwareTelemetry
```

### Regras Inegociáveis do Padrão-Alvo
1. **Regra de Escala de Arquivos:** Nenhum componente ou arquivo de hook deve ultrapassar **250 linhas**. Componentes que atingirem 200 linhas devem ser imediatamente fatiados em submódulos na pasta do domínio.
2. **Regra de Isolamento de Camada:** `components/ui/` **nunca** importa de `components/studio/`, `types/studio.ts` ou rotas. Primitivas de UI são 100% agnósticas de domínio.
3. **Regra de Named Exports:** Banidos exports mistos ou `export default` em novos componentes ou refatorações de `ui/`. Todos usam `export function NomeComponente`.
4. **The Brand-Only Rule:** Proibido uso de hexadecimais literais soltos no JSX e classes utilitárias como `emerald-*`, `rose-*`, `purple-*`. Apenas tokens `@theme` (`brand-*`, `zinc-*`, `status-*`).
5. **Critério para Criação de Componente:**
   - Se o markup e comportamento forem repetidos em 2 ou mais lugares: **extrair para `components/ui/` ou `components/composite/`**.
   - Se a lógica contiver chamadas de API, polling, cálculos matemáticos ou efeitos de janela: **extrair para um hook em `hooks/`**.
   - Se a página do App Router contiver mais de 100 linhas: a página deve atuar apenas como composição de componentes de feature.

---

## 4. Catálogo de Componentes: Existentes, Duplicados e Faltantes

### 4.1. Primitivas Existentes (`components/ui`)
- `Badge` (93 LOC) — Cápsula de status com suporte a pulso
- `Breadcrumbs` (84 LOC) — Trilha com truncamento
- `Button` (104 LOC) — Botão CTA com One CTA Rule e spinner
- `ConfirmDialog` (61 LOC) — Diálogo de confirmação (wrapper de Modal)
- `Drawer` (183 LOC) — Slide-over lateral
- `DropOverlay` (116 LOC) — Overlay de drag-and-drop
- `EmptyState` (56 LOC) — Estado vazio
- `GlassCard` (132 LOC) — Cartão de vidro composto (Header, Body, Footer)
- `Input` (84 LOC) — Campo com label mono e hint
- `Kbd` (27 LOC) — Tecla de atalho de teclado
- `MetricTile` (56 LOC) — Bloco numérico de métrica
- `Modal` (130 LOC) — Diálogo modal com hairline zenital
- `ProgressBar` (79 LOC) — Barra de progresso com variantes
- `SearchInput` (67 LOC) — Input com ícone de busca e clear button
- `SegmentedControl` (60 LOC) — Alternador de opções em pílulas
- `Select` (643 LOC) — Combobox/dropdown avançado (monólito a modularizar)
- `Slider` (64 LOC) — Input range estilizado
- `Spinner` (34 LOC) — Indicador de carregamento circular
- `StatCard` (74 LOC) — KPI card (sobreposição com MetricTile)
- `SubmodulePills` (86 LOC) — Trilho horizontal de filtros/abas
- `Toast` (101 LOC) — Barramento pub/sub com ToastHost
- `TruncatedText` (41 LOC) — Truncamento com title nativo
- `ZoomControl` (85 LOC) — Toolbar de zoom para canvas

### 4.2. Componentes Duplicados, Conflitantes ou Shims
- **`ConfirmDialog`**: Duplicado em `components/studio/ConfirmDialog.tsx` (shim de 4 linhas) vs `components/ui/ConfirmDialog.tsx`.
- **`Toast`**: Duplicado em `components/studio/Toast.tsx` (shim de 3 linhas) vs `components/ui/Toast.tsx`.
- **`CleanupJobsModal` vs `JobCleanupDialog`**: `CleanupJobsModal.tsx` (119 LOC) é código morto não utilizado em nenhum lugar do repositório; `JobCleanupDialog.tsx` (177 LOC) é o diálogo ativo.
- **`CreateDatasetModal` vs `ImportDatasetModal`**: `ImportDatasetModal.tsx` (154 LOC) é um subconjunto desatualizado de `CreateDatasetModal.tsx` (755 LOC, modo `import`).
- **`JobCard` (`JobListItem`) vs `ActionCenter` (Cards inlined)**: `ActionCenter.tsx` reimplementa 332 linhas de layout de card de job em vez de reutilizar `JobListItem`.
- **Card de Nó Orquestrador**: Duplicado entre `app/(studio)/dashboard/page.tsx:219-273` e `app/(studio)/environments/page.tsx:341-397` (clone de 55 linhas detectado pelo `jscpd`).
- **`StatCard` vs `MetricTile`**: Dois componentes em `components/ui` com propósitos quase idênticos.

### 4.3. Componentes Faltantes (Gaps Críticos que Geram Código Ad-hoc)
- **`Table` / `DataTable`**: Inexistente. Provoca a criação manual de tabelas em `DatasetTable.tsx`, `ImageTableView.tsx` e `dashboard/page.tsx`.
- **`Checkbox` / `Switch`**: Inexistente. Provoca a proliferação de mais de 15 `<input type="checkbox">` estilizados ad-hoc em formulários e modais.
- **`Tooltip`**: Inexistente. Provoca mais de 200 usos de `title="..."` nativo do navegador, que não funciona em touch e quebra WCAG.
- **`Popover`**: Inexistente como componente genérico. Está trancado dentro de `Select.tsx`.
- **`Alert` / `Callout`**: Inexistente. Provoca mais de 15 implementações manuais de banners de erro com Tailwind idêntico.
- **`Tabs` / `TabGroup`**: Inexistente. SubmodulePills provê apenas estilo de pills, sem orquestração acessível de abas.
- **`FormField`**: Inexistente. Chrome de formulário (label mono, hint, erro) duplicado entre `Input.tsx` e `Select.tsx`.
- **`Skeleton`**: Inexistente. As telas usam texto cru `<p>Carregando...</p>` causando grande Cumulative Layout Shift.
- **Ícones essenciais em `icons.tsx`**: Faltam `IconChevronLeft` e `IconChevronUp`, provocando SVG inline manual em 4 arquivos.

---

## 5. Matriz Geral de Achados e Tarefas de Engenharia

### Tabela Resumo dos Achados

| ID | Categoria | Descrição Sucinta | Evidência Principal | Impacto | Esforço | Prioridade | Cmd Impeccable |
|---|---|---|---|:---:|:---:|:---:|:---:|
| **MON-01** | Monólito | `GenerationPanel.tsx` acumula 1.846 linhas de form, img2img, polling e preview | `components/studio/GenerationPanel.tsx:1-1846` | Alto | Grande | **P0** | `extract` |
| **MON-02** | Monólito | `ForjaDifusaoSetup.tsx` acumula 1.837 linhas de presets, VRAM e form | `components/studio/ForjaDifusaoSetup.tsx:1-1837` | Alto | Grande | **P0** | `extract` |
| **MON-03** | Monólito | `ActionCenter.tsx` acumula 1.718 linhas de telemetria, jobs e 4 modais | `components/studio/ActionCenter.tsx:1-1718` | Alto | Grande | **P0** | `extract` |
| **MON-04** | Monólito | `datasets/[id]/page.tsx` acumula 1.786 linhas, 42 states e 13 modais | `app/(studio)/datasets/[id]/page.tsx:1-1786` | Alto | Grande | **P0** | `extract` |
| **MON-05** | Monólito | `jobs/page.tsx` acumula 1.598 linhas e duplica handlers com ActionCenter | `app/(studio)/jobs/page.tsx:1-1598` | Alto | Grande | **P0** | `extract` |
| **MON-06** | Monólito | `GenerationGallery.tsx` acumula 1.017 linhas com scroll infinito e lightbox | `components/studio/GenerationGallery.tsx:1-1017` | Médio | Médio | **P1** | `extract` |
| **MON-07** | Monólito | `playground/page.tsx` acumula 1.005 linhas de inferência YOLO e overlay | `app/(studio)/playground/page.tsx:1-1005` | Alto | Médio | **P1** | `extract` |
| **MON-08** | Monólito | `Select.tsx` acumula 643 linhas com motor de viewport, busca e listbox | `components/ui/Select.tsx:1-643` | Alto | Médio | **P1** | `extract` |
| **DUP-01** | Duplicação | Handlers de ciclo de vida de jobs duplicados entre Jobs e ActionCenter | `jobs/page.tsx:290-445` vs `ActionCenter.tsx:214-368` | Alto | Pequeno | **P0** | `extract` |
| **DUP-02** | Duplicação | Card e polling de nó orquestrador duplicados (55 linhas idênticas) | `dashboard/page.tsx:219` vs `environments/page.tsx:341` | Médio | Pequeno | **P1** | `extract` |
| **DUP-03** | Duplicação | Polling de hardware, cálculo de VRAM e deviceLabel clonados nas forjas | `ForjaDifusaoSetup.tsx:200` vs `ForjaYoloSetup.tsx:104` | Alto | Pequeno | **P0** | `extract` |
| **DUP-04** | Código Morto | `CleanupJobsModal.tsx` órfão e `ImportDatasetModal.tsx` redundante | `components/studio/CleanupJobsModal.tsx` | Médio | Pequeno | **P1** | `normalize` |
| **DUP-05** | Shims | `ConfirmDialog.tsx` e `Toast.tsx` em studio são re-exports desnecessários | `components/studio/ConfirmDialog.tsx:1-4` | Baixo | Pequeno | **P2** | `normalize` |
| **A11Y-01** | Acessibilidade | Ausência total de Focus Trap e Focus Restoration em `Modal` e `Drawer` | `components/ui/Modal.tsx:46` & `Drawer.tsx:60` | Alto | Pequeno | **P0** | `harden` |
| **A11Y-02** | Acessibilidade | Touch targets < 44px e remoção destrutiva de outline de foco | `components/studio/ImageCard.tsx:48` | Médio | Médio | **P2** | `polish` |
| **A11Y-03** | Acessibilidade | Contraste de texto insuficiente com `text-zinc-500` e `text-zinc-600` | > 40 arquivos (razão < 3.5:1 em dark) | Alto | Pequeno | **P1** | `polish` |
| **NEXT-01** | Next.js 16 | SPA disfarçada: 12 de 14 páginas com `"use client"`, zero loading/error | `app/(studio)/**/page.tsx` | Alto | Grande | **P1** | `harden` |
| **NEXT-02** | Next.js 16 | IP privado hardcoded em `allowedDevOrigins` no `next.config.ts` | `next.config.ts:8` | Médio | Pequeno | **P2** | `harden` |
| **NEXT-03** | Next.js 16 | Tratamento de 401 fragmentado: 28+ replicações de `router.replace("/login")` | > 20 componentes e páginas | Alto | Pequeno | **P1** | `harden` |
| **TOK-01** | Estilos | Cores não-canônicas (`rose-*`, `sky-*`, `blue-*`, `emerald-200`) e 67 hex soltos | `app/globals.css` e 38 componentes | Médio | Médio | **P2** | `normalize` |
| **TOK-02** | Estilos | Camada `z-50` saturada com conflito entre modais, toasts e selects | `globals.css` / 15 arquivos | Alto | Pequeno | **P1** | `normalize` |
| **TYP-01** | Contratos | Monólito `types/studio.ts` (829 LOC) e 53 casts inseguros `job.params as any` | `types/studio.ts:1-829` | Médio | Médio | **P1** | `normalize` |
| **TYP-02** | Utilitários | Implementações duplicadas de `formatBytes` (4 arquivos diferentes) | `lib/format.ts` vs 3 modais de studio | Baixo | Pequeno | **P3** | `normalize` |
| **UI-01** | Primitivas | Criação das primitivas faltantes (`Table`, `Checkbox`, `Switch`, `Tooltip`, etc.) | `components/ui/*` | Alto | Médio | **P1** | `extract` |

---

### Detalhamento Completo das Tarefas com Evidência e Critérios de Aceite

#### [TASK-WEB-001] Extração do Hook Canônico de Ciclo de Vida de Jobs (`useJobLifecycle`)
- **Evidência:** `apps/web/app/(studio)/jobs/page.tsx:290-445` e `apps/web/components/studio/ActionCenter.tsx:214-368` (mais de 300 linhas de código idêntico).
- **Problema:** Lógica de cancelamento de job (`abortJob`), exclusão (`deleteJob`), aplicação de predições (`applyAutotrackerBoxes`), aplicação de legendas (`applyAutolabelCaptions`) e download de artefatos duplicada integralmente com os mesmos blocos `try/catch` e mensagens de toast.
- **Proposta:** Criar `apps/web/hooks/useJobLifecycle.ts` encapsulando todas as mutações e consumi-lo em ambos os arquivos.
- **Critério de Aceite:**
  1. Ambas as telas (`/jobs` e `ActionCenter`) consomem exclusivamente o hook.
  2. Nenhuma menção a `abortJob` ou `deleteJob` direto nas páginas.
  3. Redução de pelo menos 250 linhas combinadas.
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-002] Extração dos Hooks de Telemetria e Estimativa de VRAM (`useHardwareTelemetry` e `useVramEstimator`)
- **Evidência:** `ForjaDifusaoSetup.tsx:200-240, 712-728` vs `ForjaYoloSetup.tsx:104-169`.
- **Problema:** O polling da GPU via `getTelemetry()`, o fallback para mock/CPU, a conversão de bytes para GB e o cálculo de risco de estouro de VRAM (`oomRisk: "safe" | "warning" | "danger"`) foram copiados e colados entre a Forja de Difusão e a Forja de YOLO.
- **Proposta:** Criar `apps/web/hooks/useHardwareTelemetry.ts` (polling com `visibilitychange`) e `apps/web/hooks/useVramEstimator.ts` (cálculo de risco puro).
- **Critério de Aceite:**
  1. `ForjaDifusaoSetup.tsx` e `ForjaYoloSetup.tsx` utilizam os mesmos hooks.
  2. Eliminação de 150 linhas de duplicação.
  3. Comportamento idêntico de exibição do alerta de risco de VRAM.
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-003] Blindagem de Acessibilidade em `Modal.tsx` e `Drawer.tsx` (Focus Trap & Scroll Lock)
- **Evidência:** `apps/web/components/ui/Modal.tsx:46-51` e `apps/web/components/ui/Drawer.tsx:60-75`.
- **Problema:** Modais e Drawers não aprisionam o foco do teclado (o usuário pode navegar para elementos escondidos atrás do backdrop com `Tab`), não devolvem o foco para o botão disparador ao fechar, e `Modal.tsx` não trava o scroll da página de fundo. `ConfirmDialog.tsx` não expõe `role="alertdialog"`.
- **Proposta:** Criar `hooks/useFocusTrap.ts` e `hooks/useBodyScrollLock.ts` (com contador de referências em stack), integrando-os diretamente em `Modal.tsx`, `Drawer.tsx` e `ConfirmDialog.tsx`.
- **Critério de Aceite:**
  1. Ao abrir um modal/drawer, o foco inicial vai para o diálogo.
  2. Pressionar `Tab` sucessivamente navega apenas dentro do modal em loop cíclico.
  3. Ao fechar com `Escape` ou clique, o foco retorna exatamente ao elemento disparador.
  4. O scroll da página ao fundo é travado sem provocar saltos de largura (layout shift).
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-004] Decomposição do Super-Monólito `GenerationPanel.tsx` (1.846 LOC)
- **Evidência:** `apps/web/components/studio/GenerationPanel.tsx:1-1846`.
- **Problema:** Arquivo gigante concentrando 28 `useState`, 6 `useRef`, polling via `setInterval`, listeners de `BroadcastChannel`, manipulação de `localStorage`, upload de imagem img2img com controle de sequência, cálculo vetorial de aspect ratio e renderização de controles e visualizador.
- **Proposta:** Criar pasta `apps/web/components/studio/generation/` e extrair:
  1. `GenerationPanel.tsx` (~180 LOC — shell)
  2. `GenerationControls.tsx` (~220 LOC — acordeons de steps, CFG, seed, samplers)
  3. `GenerationPromptBar.tsx` (~140 LOC — prompts positivo/negativo)
  4. `GenerationImg2ImgDropzone.tsx` (~180 LOC — dropzone efêmero)
  5. `GenerationViewport.tsx` (~210 LOC — visualizador interativo com zoom)
  6. `hooks/useGenerationFormPersist.ts` (~120 LOC — sincronização com storage)
- **Critério de Aceite:**
  1. Nenhum arquivo resultante ultrapassa 250 linhas.
  2. Persistência de formulário e atalhos de teclado (Ctrl+Enter para gerar) preservados.
- **Esforço:** Grande | **Risco:** Médio | **Dependências:** TASK-WEB-003.

#### [TASK-WEB-005] Decomposição do Super-Monólito `ForjaDifusaoSetup.tsx` (1.837 LOC)
- **Evidência:** `apps/web/components/studio/ForjaDifusaoSetup.tsx:1-1837`.
- **Problema:** Concentra setup completo de treino de difusão LoRA, importação/exportação de JSON de presets, cálculo de VRAM, seletores de quantização e dezenas de inputs manuais.
- **Proposta:** Criar pasta `apps/web/components/studio/diffusion/` e extrair:
  1. `ForjaDifusaoSetup.tsx` (~160 LOC — orquestração)
  2. `DiffusionPresetBar.tsx` (~180 LOC — botões de presets e import/export)
  3. `DiffusionBaseModelSelector.tsx` (~150 LOC — SD15, SDXL, FLUX, text encoder)
  4. `DiffusionHyperparametersFields.tsx` (~200 LOC — épocas, batch, lr, rank)
  5. `DiffusionAdvancedSettings.tsx` (~220 LOC — otimizador, precisão, caching)
  6. `DiffusionValidationSamples.tsx` (~140 LOC — amostras de validação)
- **Critério de Aceite:**
  1. Nenhum arquivo resultante ultrapassa 250 linhas.
  2. Consome `useHardwareTelemetry` e `useVramEstimator`.
  3. Presets JSON importados continuam funcionando sem regressão.
- **Esforço:** Grande | **Risco:** Médio | **Dependências:** TASK-WEB-002.

#### [TASK-WEB-006] Decomposição do Super-Monólito `ActionCenter.tsx` (1.718 LOC)
- **Evidência:** `apps/web/components/studio/ActionCenter.tsx:1-1718`.
- **Problema:** Drawer lateral com polling pesado a cada 3s, renderização de cards de job inlined (duplicando `JobCard.tsx`), telemetria inlined e montagem de 4 modais aninhados.
- **Proposta:** Criar pasta `apps/web/components/studio/action-center/` e extrair:
  1. `ActionCenter.tsx` (~190 LOC — drawer e abas)
  2. `ActionCenterJobList.tsx` (~220 LOC — lista reutilizando `JobListItem` de `JobCard.tsx`)
  3. `ActionCenterNotifications.tsx` (~160 LOC — feed de avisos de infraestrutura)
  4. `ActionCenterTelemetryFooter.tsx` (~120 LOC — rodapé de hardware)
  5. `hooks/useActionCenterPolling.ts` (~140 LOC — polling isolado)
- **Critério de Aceite:**
  1. `ActionCenter` reutiliza `JobListItem` de `JobCard.tsx` eliminando a duplicação de layout de jobs.
  2. Nenhum arquivo resultante ultrapassa 250 linhas.
  3. Consome `useJobLifecycle` (TASK-WEB-001).
- **Esforço:** Grande | **Risco:** Médio | **Dependências:** TASK-WEB-001.

#### [TASK-WEB-007] Decomposição da Galeria de Datasets `datasets/[id]/page.tsx` (1.786 LOC)
- **Evidência:** `apps/web/app/(studio)/datasets/[id]/page.tsx:1-1786`.
- **Problema:** Maior monólito de rota do frontend. Acumula 42 hooks `useState`, 7 `useRef`, observador de scroll infinito, polling de busca CLIP, upload multipart chunked e orquestração de 13 modais inline.
- **Proposta:**
  1. Extrair `hooks/useDatasetGallery.ts` (carregamento, paginação e seleção em lote).
  2. Extrair `hooks/useClipSearch.ts` (busca semântica e polling de indexação).
  3. Extrair `hooks/useDatasetUpload.ts` (drag-and-drop e upload em lote).
  4. Mover a orquestração de modais para componentes contextuais separados em `components/studio/datasets/`.
  5. Reduzir a página a ~180 linhas de composição pura.
- **Critério de Aceite:**
  1. A página `page.tsx` fica com menos de 200 linhas.
  2. Uploads, seleções em lote e busca semântica permanecem intactos.
- **Esforço:** Grande | **Risco:** Alto | **Dependências:** TASK-WEB-003.

#### [TASK-WEB-008] Decomposição do Editor de Anotações `annotate/[imageId]/page.tsx` (806 LOC)
- **Evidência:** `apps/web/app/(studio)/datasets/[id]/annotate/[imageId]/page.tsx:1-806`.
- **Problema:** Mistura o motor vetorial 2D (conversão tela/normalizada, matemática de zoom e pan, tracking de ponteiro em listeners globais de window), captura de atalhos de teclado (B, V, H, Del, 1-9, Esc), autosave com debounce e reconciliação de IDs de bounding box com a renderização de UI da página.
- **Proposta:**
  1. Extrair `hooks/useAnnotationCanvas.ts` (toda a matemática de zoom, pan, coordenadas e listeners de mouse/touch).
  2. Extrair `hooks/useAnnotationSync.ts` (debounce de autosave e reconciliação com o backend).
  3. Criar componentes `AnnotationSidebar.tsx` e `AnnotationCanvasView.tsx`.
- **Critério de Aceite:**
  1. A rota `page.tsx` fica com menos de 150 linhas.
  2. Precisão sub-pixel e desenho de caixas YOLO 100% mantidos.
- **Esforço:** Médio | **Risco:** Médio | **Dependências:** Nenhuma.

#### [TASK-WEB-009] Desacoplamento e Modularização de `Select.tsx` (643 LOC)
- **Evidência:** `apps/web/components/ui/Select.tsx:1-643`.
- **Problema:** Primitiva de UI monólita que implementa seu próprio motor de cálculo de viewport flutuante, teclado cíclico de listbox, portal para `document.body` e barra de busca integrada.
- **Proposta:**
  1. Extrair `hooks/useFloatingPosition.ts` (cálculo de flip e posicionamento flutuante reutilizável para Tooltip e Popover).
  2. Extrair `hooks/useListboxNavigation.ts` (máquina de estados de setas de teclado).
  3. Extrair `components/ui/FormField.tsx` (chrome de formulário compartilhado).
  4. Reduzir `Select.tsx` para ~150 linhas focadas na composição.
- **Critério de Aceite:**
  1. `Select.tsx` com menos de 180 linhas.
  2. Suporte a busca e navegação por teclado (ArrowUp/ArrowDown/Enter) 100% preservados.
  3. `useFloatingPosition` reutilizado na nova primitiva `Tooltip.tsx`.
- **Esforço:** Médio | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-010] Unificação de Nós Orquestradores (`OrchestratorCard`) e Eliminação do Clone
- **Evidência:** `app/(studio)/dashboard/page.tsx:219-273` vs `app/(studio)/environments/page.tsx:341-397` (clone jscpd de 55 linhas e 304 tokens).
- **Problema:** Cards visuais de nós orquestradores (badges com ping dot, gauges de GPU/VRAM/CPU, fallback offline) foram duplicados textualmente entre o Dashboard e a tela de Ambientes.
- **Proposta:** Criar `components/composite/OrchestratorCard.tsx` e consumi-lo em ambas as telas.
- **Critério de Aceite:**
  1. Clone eliminado (0 linhas duplicadas no jscpd para este bloco).
  2. Ações de adoção/revogação passadas por slots ou callbacks opcionais.
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-011] Limpeza de Código Morto, Shims e Duplicações Obsoletas
- **Evidência:**
  - `components/studio/CleanupJobsModal.tsx` (119 LOC — 0 imports em todo o repo).
  - `components/studio/ImportDatasetModal.tsx` (154 LOC — redundante com `CreateDatasetModal`).
  - `components/studio/ConfirmDialog.tsx` (4 LOC — shim).
  - `components/studio/Toast.tsx` (3 LOC — shim).
- **Problema:** Arquivos mortos e shims geram poluição de busca, caminhos de importação concorrentes e confusão na manutenção.
- **Proposta:**
  1. Deletar `CleanupJobsModal.tsx`.
  2. Migrar os 3 arquivos que usam `components/studio/ConfirmDialog.tsx` para `@/components/ui/ConfirmDialog` e deletar o shim.
  3. Migrar os 17 arquivos que usam `components/studio/Toast.tsx` para `@/components/ui/Toast` e deletar o shim.
  4. Consolidar importação de datasets exclusivamente em `CreateDatasetModal.tsx` (modo `import`) e deletar `ImportDatasetModal.tsx`.
- **Critério de Aceite:**
  1. Remoção de 4 arquivos sem quebrar nenhum import nem o `tsc --noEmit`.
  2. Redução de 280 linhas de código desnecessário.
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-012] Criação das Primitivas de UI Faltantes no Design System
- **Evidência:** Proliferação de tabelas ad-hoc (`DatasetTable`, `ImageTableView`, `dashboard`), switches manuais (`ForjaDifusaoSetup`, `ActionCenter`), tooltips nativos (`title="..."`), e banners de alerta copiados.
- **Problema:** Ausência de primitivas essenciais força desenvolvedores e agentes a inventarem marcação e estilos ad-hoc em cada tela.
- **Proposta:** Implementar em `components/ui/`:
  1. `Table.tsx` (`Table`, `TableHeader`, `TableBody`, `TableRow`, `TableHead`, `TableCell`).
  2. `Checkbox.tsx` (com estado indeterminado e foco acessível).
  3. `Switch.tsx` (`role="switch"`, `aria-checked`).
  4. `Tooltip.tsx` (óptico Arcane UI com `useFloatingPosition`).
  5. `Alert.tsx` (`role="alert"` com variantes `info`, `warning`, `danger`).
  6. `FormField.tsx` (`label`, `hint`, `error` com IDs correlacionados).
- **Critério de Aceite:**
  1. Primitivas criadas com tipagem estrita, acessibilidade e conformidade a `docs/DESIGN.md`.
  2. Substituição de marcações ad-hoc em pelo menos 3 telas de estúdio.
- **Esforço:** Médio | **Risco:** Baixo | **Dependências:** TASK-WEB-009 (`useFloatingPosition`).

#### [TASK-WEB-013] Normalização de Tokens `@theme`, Escala de z-index e Limpeza de Cores
- **Evidência:**
  - 67 valores hexadecimais soltos em 15 arquivos (incluindo `#a7f3d0` hardcoded em 7 componentes).
  - Mais de 38 arquivos contaminados com classes utilitárias fora da paleta (`rose-*`, `sky-*`, `blue-*`, `indigo-*`).
  - Camada `z-50` saturada gerando sobreposição indevida entre modais, gavetas e toasts.
- **Proposta:**
  1. Adicionar escala declarativa de z-index em `apps/web/app/globals.css` no `@theme`:
     - `--z-dropdown: 100;`
     - `--z-sticky: 200;`
     - `--z-drawer: 300;`
     - `--z-modal: 400;`
     - `--z-popover: 500;`
     - `--z-toast: 600;`
  2. Criar token canônico para texto sobre véu de sucesso (substituindo `#a7f3d0`).
  3. Substituir `rose-*` por `status-danger`, `sky-*`/`blue-*` por `status-telemetry` e `indigo-*`/`purple-*` por `brand-*`.
- **Critério de Aceite:**
  1. Zero conflitos de z-index (toasts sempre sobrepõem modais e drawers).
  2. Redução de pelo menos 80% dos hexadecimais soltos no JSX.
- **Esforço:** Médio | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-014] Modularização de `types/studio.ts` e Tipagem Segura de Jobs
- **Evidência:** `apps/web/types/studio.ts:1-829` (829 LOC monólito) e 53 ocorrências de casts inseguros `(job.params as any)` em `jobs/page.tsx:28` e `ActionCenter.tsx:25`.
- **Problema:** Monólito mistura todos os domínios em um único arquivo, possui definições duplicadas internamente e tipa `Job.params` de forma genérica, forçando dezenas de type casts forçados que escondem erros de compilação.
- **Proposta:**
  1. Criar pasta `apps/web/types/` e particionar em:
     - `datasets.ts`, `jobs.ts`, `diffusion.ts`, `yolo.ts`, `models.ts`, `orchestrators.ts`, `telemetry.ts`.
  2. Transformar `Job` em Tagged Union ou tipar `Job.params` como `DiffusionJobParams | YoloJobParams | AutolabelJobParams | AutotrackerJobParams`.
  3. Manter `apps/web/types/index.ts` como barrel re-exportando tudo para 100% de compatibilidade reversa.
  4. Centralizar tradução de erros em `lib/errors.ts` e formatadores em `lib/format.ts`.
- **Critério de Aceite:**
  1. Eliminação dos 53 casts `(job.params as any)`.
  2. Nenhum arquivo em `types/` ultrapassa 200 linhas.
  3. Zero quebra em arquivos que importam de `@/types/studio`.
- **Esforço:** Médio | **Risco:** Baixo | **Dependências:** Nenhuma.

#### [TASK-WEB-015] Restauração da Arquitetura App Router (Server Components & Streaming)
- **Evidência:** 68 de 107 arquivos com `"use client"`. Zero arquivos `loading.tsx`, `error.tsx` e `not-found.tsx`. `app/(studio)/layout.tsx` inteiramente client-side.
- **Problema:** A aplicação não aproveita SSR, streaming de HTML, SEO de metadata nem Error Boundaries por rota.
- **Proposta:**
  1. Transformar `app/(studio)/layout.tsx` em Server Component, isolando o estado de abertura da Sidebar e ActionCenter em um wrapper fino de contexto client (`StudioShellProvider`).
  2. Criar `app/(studio)/loading.tsx` com esqueleto de carregamento óptico Arcane.
  3. Criar `app/(studio)/error.tsx` com captura de falha de renderização e botão de recuperação.
  4. Criar `app/(studio)/not-found.tsx` integrado à estética do estúdio.
  5. Migrar as páginas de catálogo (`/datasets`, `/models`, `/environments`) para Server Components que executam fetch inicial dos dados e passam como props para Client Islands de visualização e filtro.
- **Critério de Aceite:**
  1. `app/(studio)/layout.tsx` roda no servidor (sem `"use client"`).
  2. Navegação instantânea com skeleton durante transições lentas de rede.
  3. Erros inesperados de renderização são contidos em `error.tsx` sem tela branca.
- **Esforço:** Grande | **Risco:** Médio | **Dependências:** TASK-WEB-012 (Skeleton).

#### [TASK-WEB-016] Centralização do Tratamento de 401 e Limpeza de `next.config.ts`
- **Evidência:** Mais de 28 ocorrências de `router.replace("/login")` dispersas em páginas e modais, e IP privado fixo em `next.config.ts:8` (`allowedDevOrigins: ["10.15.10.3"]`).
- **Problema:** Quando a sessão expira, cada componente precisa tratar o erro 401 individualmente, gerando duplicação e risco de loops. O IP hardcoded quebra portabilidade para outros desenvolvedores ou ambientes de rede.
- **Proposta:**
  1. Configurar `lib/api.ts` para interceptar respostas 401: disparar evento global ou redirecionar centralizadamente para `/login?expired=1`.
  2. Parametrizar `allowedDevOrigins` no `next.config.ts` via variável de ambiente `DEV_ALLOWED_ORIGIN`.
- **Critério de Aceite:**
  1. Remoção de blocos repetidos de redirecionamento 401 nos componentes de estúdio.
  2. Configuração de build limpa e dinâmica.
- **Esforço:** Pequeno | **Risco:** Baixo | **Dependências:** Nenhuma.

---

## 6. Roadmap de Execução em Fases Independentes

O plano foi estruturado em **6 fases atômicas**. Cada fase representa um Pull Request independente, plenamente verificável por testes e builds verdes (`biome check`, `tsc --noEmit`), sem alterar a API pública nem causar quebras visuais.

```
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ ROADMAP DE MODULARIZAÇÃO — APPS/WEB                                                    │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ FASE 1: Limpeza Rápida, Eliminação de Shims & Resolução de Código Morto  (Quick Wins) │
│         • Deletar CleanupJobsModal.tsx e ImportDatasetModal.tsx                        │
│         • Eliminar shims Toast.tsx e ConfirmDialog.tsx em studio                       │
│         • Corrigir IP hardcoded no next.config.ts e unificar formatBytes               │
│                                                                                        │
│ FASE 2: Primitivas Faltantes, Acessibilidade (A11y) & Tokens `@theme`                  │
│         • Hook useFocusTrap e useBodyScrollLock em Modal e Drawer                      │
│         • Primitivas: Table, Checkbox, Switch, Tooltip, Alert, FormField               │
│         • Escala declarativa de z-index em globals.css e saneamento de cores           │
│                                                                                        │
│ FASE 3: Camada de Domínio Compartilhada (Hooks & Contratos OpenAPI)                   │
│         • Extrair useJobLifecycle, useHardwareTelemetry e useVramEstimator             │
│         • Criar OrchestratorCard eliminando clone de 55 linhas                         │
│         • Modularizar types/studio.ts por domínio com união tipada em Job.params       │
│                                                                                        │
│ FASE 4: Fatiamento dos Super-Monólitos de Estúdio (Arquivos < 250 LOC)                 │
│         • Decompor GenerationPanel.tsx (1.846 LOC ➔ 6 submódulos)                      │
│         • Decompor ForjaDifusaoSetup.tsx (1.837 LOC ➔ 6 submódulos)                    │
│         • Decompor ActionCenter.tsx (1.718 LOC ➔ 5 submódulos)                         │
│         • Decompor ConvergenceChart.tsx (690 LOC ➔ 3 submódulos)                       │
│                                                                                        │
│ FASE 5: Fatiamento e Desacoplamento das Rotas de Datasets & YOLO                       │
│         • Decompor datasets/[id]/page.tsx (1.786 LOC) com hooks de galeria/upload     │
│         • Decompor annotate/[imageId]/page.tsx (806 LOC) com useAnnotationCanvas       │
│         • Decompor playground/page.tsx (1.005 LOC) e models/page.tsx                   │
│                                                                                        │
│ FASE 6: Restauração da Arquitetura Next.js 16 (Server Components & Polimento)          │
│         • Transformar layout.tsx em Server Component com StudioShellProvider           │
│         • Criar loading.tsx (skeletons), error.tsx e not-found.tsx                     │
│         • Interceptor 401 centralizado em lib/api.ts                                   │
│         • Sincronizar docs/web/architecture.md com as rotas reais implementadas        │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. O Que NÃO Mudar e Riscos Arquiteturais

### O Que NÃO Mudar (Preservação de Invariantes)
1. **Identidade Visual Arcane v2:** Não alterar o tom do acento brand `#8350f2`, as superfícies em vidro óptico (.glass-card, .glass-menu, .glass-modal), nem a tipografia Space Grotesk / JetBrains Mono. O objetivo é padronizar e modularizar, não redesign.
2. **Rewrites Transparentes de Proxy:** Manter a mecânica do `next.config.ts` que encaminha `/api/*` para o BFF Rust (`api-principal` na porta `:8080`). O browser nunca deve chamar portas internas diretamente.
3. **Mecânica de Upload de Grandes Volumes:** Preservar `proxyClientMaxBodySize: "8200mb"` e `proxyTimeout: 900_000`. O estúdio transaciona checkpoints de pesos de difusão de múltiplos gigabytes.
4. **Convenção de Casing Wire:** Manter estritamente `camelCase` em todos os contratos com a API REST pública, respeitando o contrato OpenAPI.
5. **Autonomia Local-First:** Não introduzir dependências de serviços de nuvem ou telemetria externa proprietária no frontend.

### Riscos Mapeados & Estratégias de Mitigação
- **Risco 1: Regressão no Canvas de Anotação YOLO (`annotate/[imageId]`):**
  - *Perigo:* A conversão de coordenadas tela-para-normalizado (`x1, y1, x2, y2`) e zoom/pan pode quebrar durante a extração do hook.
  - *Mitigação:* Isolar a matemática de coordenadas como funções puras desacopladas do React e testá-las de forma determinística antes de refatorar o componente visual.
- **Risco 2: Quebra de Persistência no Formulário de Geração:**
  - *Perigo:* Usuários perdem prompts longos e configurações ao navegar entre rotas.
  - *Mitigação:* Manter a chave `localStorage` e a estrutura de dados existente em `lib/geracao-storage.ts`, garantindo retrocompatibilidade total com snapshots antigos.
- **Risco 3: Conflito de Imports por quebra de `types/studio.ts`:**
  - *Perigo:* Quebrar compilação de dezenas de arquivos ao mover tipos para subarquivos.
  - *Mitigação:* `apps/web/types/index.ts` e `apps/web/types/studio.ts` devem re-exportar integralmente todos os tipos das novas fatias modulares.

---

## 8. Texto Sugerido de "Convenções de UI" para o `AGENTS.md`

*(Proposta estritamente consultiva para futura inclusão no AGENTS.md pelo coordenador; não edite o arquivo agora).*

```markdown
## Convenções de UI & Engenharia Frontend (`apps/web`)

1. **Limite Estrito de Complexidade:** Nenhum componente, página ou hook deve exceder **250 linhas de código**. Ao atingir 200 linhas, fatie imediatamente em submódulos na pasta da feature.
2. **Hierarquia de Componentes & Camadas:**
   - `components/ui/`: Primitivas 100% agnósticas de domínio. Proibido importar de `studio/`, `types/` de domínio ou rotas. Named exports estritos (proibido `export default`).
   - `components/composite/`: Blocos reutilizáveis multirrecursos (ex: `OrchestratorCard`, `JobMetricChips`).
   - `components/studio/<feature>/`: Componentes específicos de negócio (treino, datasets, geração, jobs).
   - `app/(studio)/*/page.tsx`: Páginas operam exclusivamente como composição de blocos e carregamento de dados (<150 linhas).
3. **Separação de Preocupações:**
   - Proibido inlining de polling (`setInterval`), manipulação de hardware/VRAM ou mutações de API dentro do JSX. Toda lógica reside em `hooks/` ou `lib/`.
4. **The Brand-Only Rule:**
   - Proibido uso de hexadecimais literais soltos no JSX (`#[0-9a-fA-F]`).
   - Proibido uso de cores utilitárias fora da identidade (`rose-*`, `sky-*`, `blue-*`, `emerald-*`). Utilize unicamente tokens `@theme` (`brand-*`, `zinc-*`, `status-*`).
5. **Acessibilidade Mandatória (WCAG 2.2 AA):**
   - Modais e Drawers obrigatoriamente utilizam `useFocusTrap`, `useBodyScrollLock` e restauram o foco no fechamento.
   - Diálogos de confirmação ou destrutivos exigem `role="alertdialog"`.
   - Proibido remover anel de foco (`focus:outline-none` sem anel `ring-brand-500` visível).
   - Elementos de clique interativo devem respeitar a área de toque mínima de 44x44px.
6. **Contratos & Tipagem Estrita:**
   - Proibido uso de `any` ou type casts inseguros `(job.params as any)`. Utilize as uniões tipadas canônicas de `types/`.
```
