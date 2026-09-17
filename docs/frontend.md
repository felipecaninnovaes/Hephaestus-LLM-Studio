# Hephaestus LLM Studio — Documentação do Front-end

> Fonte: o próprio app (`apps/web`) + `docs/DESIGN.md` (Design System unificado Arcane v2/v2.1) + `apps/web/components/ui/README.md`. Protótipos legados foram completamente aposentados e removidos do repositório.
> Status: **shell v2.1 implementado: Sidebar macro pinável + breadcrumbs dinâmicos + Centro de Atividades (ActionCenter); Painel de Controle (/dashboard) como landing page padrão; biblioteca de componentes atômicos em components/ui/. Alvo real: Next.js 16 + React 19 + TypeScript**.
> Idioma da UI: pt-BR.

## 1. Visão geral e posição na arquitetura

Fluxo definido em `IDEIA.md` e no diagrama:

```
Interface (Next.js/TS, abas por tipo de modelo)
  → Backend principal (Rust: API central, arquivos upload/download, JSON/YAML de config)
  → Orquestrador (Rust: decide local vs remoto)
  → Motor de treino (Python: PyTorch e outros engines)
      → GPU local | VPS / RunPod
```

Responsabilidades exclusivas do front-end (cap. 1 da IDEIA):

- Abas separadas por tipo de modelo/tarefa.
- Visualização e upload de imagens.
- Telas de AutoLabel e AutoTracker (com upload de modelo customizado).
- Datasets em lista ou grade; **usuário precisa clicar no dataset para abrir a galeria** e só lá acessa AutoLabel / AutoTracker / Exportar / Importar (backup estruturado).

O front-end nunca executa treino, nunca decide onde treinar, nunca manipula arquivo diretamente — tudo via API Rust.

## 2. Protótipo atual vs. alvo

| Aspecto | Protótipo v1 (APOSENTADO — comparativo histórico) | Alvo Next.js/TS |
|---|---|---|
| Runtime | React 18 UMD + Babel standalone + Tailwind CDN, tudo num `App()` com ~40 `useState` | App Router, componentes server/client separados, Tailwind real + CSS modules |
| Estado | Local, mockado (`INITIAL_DATASETS`, `setInterval` de 3s simulando epoch) | Server state via React Query / SWR + client state via Zustand; jobs reais via polling/WS |
| Gráficos | SVG estático com paths hardcoded (`lossGrad`, `mapGrad`, `clipGrad`) | Recharts/ECharts ou SVG próprio alimentado por `/api/jobs/:id/metrics` |
| Canvas BBox | `div`s absolutas simulando caixas | Canvas real (Fabric/Konva ou `<canvas>` próprio) com coordenadas normalizadas 0-1 |
| Upload | Botões que só disparam `showToast` | `multipart/form-data` → backend Rust, com resultado POR ITEM (`stored/duplicate/rejected/failed` + `reason`; sem resume na 3b — ADR-0003 D2) |
| Logs | Array de strings com `slice(-50)` | Stream WebSocket `Rust Core → Orquestrador → Motor Python` |
| i18n/a11y | pt-BR hardcoded, bom ponto de partida a11y | Manter padrão + extrair strings |

## 3. Design system implementado (Arcane v2/v2.1)

A especificação normativa completa e canônica vive em **`docs/DESIGN.md`**. Principais diretrizes:

- **Paleta dark-only profunda:** fundo base `#0d0d0d`, acento primário `brand-500` Violeta Arcane (`#8350f2`), neutros `zinc-*` com undertone berinjela, secundários semânticos (`#34d399` sucesso, `#f59e0b` alerta, `#ef4444` perigo, `#06b6d4` telemetria).
- **Regra Brand-Only:** classes `emerald-*` são **estritamente proibidas** no app (no Tailwind v4 nativo resolvem para verde legado da v1).
- **Vidro óptico em 3 níveis:** `.glass-menu` (dropdowns, toast, context menu), `.glass-card` (cards, gráficos), `.glass-modal` (modais). Todos com `backdrop-filter: blur + saturate`, borda superior zenital (`border-top: 1px solid rgba(255, 255, 255, 0.16)`) e sombra profunda.
- **Tipografia:** `Space Grotesk` para display/títulos, system sans para UI e `JetBrains Mono` para números, configs, telemetria, logs e BBoxes (regra *Monospace Truth*). Fontes self-hosted em `apps/web/fonts/*.woff2`.
- **Densidade:** `html { font-size: 14px }` (densidade compacta profissional Arcane).
- **CTA único translúcido:** exatamente 1 botão por contexto no formato `border-brand-500/30 bg-brand-500/[0.12] text-white` (regra *One CTA*). Botão com fundo sólido `bg-brand-500` é proibido.
- **Feedback:** toast bottom-right (`studio-toast`, `role=status aria-live=polite`, auto-dismiss), menu de contexto em vidro (`glass-context-menu`), modais acessíveis com `role=dialog aria-modal=true`, foco inicial e `Escape`.
- **A11y:** `focus-visible` violeta 2px (`outline: 2px solid #8350f2`), `button:disabled {opacity .55}`, `prefers-reduced-motion: reduce` zerando animações, labels semânticos em formulários.
- **Scrollbars finas 5px** e telemetria em `font-mono text-xs`.

## 4. Shell global (v2.1 — Arcane Foundry)

> O shell v2.1 é composto por `Sidebar.tsx` (pinável no desktop e drawer móvel) + header com breadcrumbs dinâmicos e Centro de Atividades (`ActionCenter.tsx`) + `ToastHost` + fundo óptico contínuo (iluminação radial violeta Arcane, grade técnica SVG 48px e vignette).

### 4.1 Sidebar macro (`components/studio/Sidebar.tsx`)

- **Navegação estruturada em 4 seções temáticas:**
  1. *Estúdio & Dados:* **Painel** (`/dashboard`, ativo como home, ícone `IconHome`), **Datasets** (`/datasets`, ativo, ícone `IconDatabase`).
  2. *Treinamento & Execução:* **Treino YOLO** (`/treino`, rota nova F6.3 — setup de treino YOLO central), **Execuções** (`/jobs`, renomeada F6.3 — fila de trabalho + histórico agrupado ativos primeiro + painel de detalhe; badge numérico de `telemetry.jobsActive` em tempo real).
     3. *Forja & Engenharia:* **Difusão LoRA** (`/difusao`, badge "Roadmap", desabilitado honesto), **OpenCLIP** (`/openclip`, badge "Roadmap", desabilitado), **Geração** (`/geracao`, ativo — novo, ADR-0023 D0), **Detecção YOLO** (`/playground`, ativo — renomeado ADR-0023 D0, badge Roadmap removido Fatia J; ADR-0013 D7), **Modelos & Pesos** (`/models`, ativo — badge Roadmap removido, Fatia I; ADR-0012 D7).
  4. *Infraestrutura:* **Orquestradores** (`/environments`, ativo — badge Roadmap removido, chip multi-nó "N orquestradores"), **Storage S3** (`/storage`, badge "Roadmap", desabilitado).
  5. *Sistema:* **Registro de Logs** (`/events`, badge "Roadmap", desabilitado), **Configurações** (`/settings`, badge "Roadmap", desabilitado).
- **Destaque de rota ativa:** derivado dinamicamente via `usePathname()`, aplicando borda violeta `border-brand-500/30 bg-zinc-900/90` com barra lateral indicadora `bg-brand-500`.
- **Pin da Sidebar no Desktop:** no viewport `lg`, a sidebar suporta modo fixado/expandido (`260px`) ou recolhido (`68px` exibindo apenas ícones com tooltips), alternado pelo botão de pino no rodapé e persistido em `localStorage` (`hephaestus_sidebar_pinned`). Um espaçador estático com transição suave em `layout.tsx` previne layout-shift durante a expansão.
- **Drawer móvel:** em viewports menores que `lg`, colapsa automaticamente em drawer retrátil (`w-[min(85vw,320px)]`) com backdrop escurecido (`bg-black/70 backdrop-blur-sm`), fechando com tecla `Escape` ou ao clicar fora/navegar.
- **Card TELEMETRIA DO NÓ REAL:** polling contínuo `getTelemetry()` a cada 3s (com auto-pausa via `visibilitychange`), monitorando consumo de VRAM com barra percentual, CPU, RAM em GB e GPUs ativas, além do contador de jobs ativos no cabeçalho.
- **Ações de rodapé:** botão direto para abertura do Centro de Atividades (`onOpenActionCenter`), botão de alternância de fixação da sidebar e botão **Sair** (`POST /api/auth/logout` + redirect `/login`).

### 4.2 Header do shell (`app/(studio)/layout.tsx`)

- Botão hambúrguer móvel para abertura da Sidebar (`lg:hidden`).
- **Breadcrumbs dinâmicos:** gerados automaticamente via `usePathname()` com mapeamento amigável (`SEGMENT_LABELS`: dashboard → "Painel", datasets → "Datasets", annotate → "Anotar", treino → "Treino YOLO", jobs → "Execuções", models → "Modelos", geracao → "Geração", login → "Login"), com truncamento intermediário em `max-w-[140px]` e destaque semi-bold no item ativo.
- **Botão do Centro de Atividades:** atalho de alta visibilidade no canto superior direito com ícone de raio (`IconZap` em `text-brand-400`), abrindo o drawer lateral de operações e monitoramento.
- **Chip de Ambiente / Nó:** cápsula de status `Badge` variante `telemetry` com ponto luminoso pulsante exibindo "Local" (`title="Nó local — ambiente único nesta fatia"`).

### 4.3 Centro de Atividades (`components/studio/ActionCenter.tsx`)

- **Painel lateral deslizante (`Drawer` à direita):** overlay retrátil para supervisão operacional ininterrupta em qualquer tela do estúdio.
- **Abas de filtragem:**
  - `Todos`: visão consolidada de jobs, telemetria e avisos de sistema.
  - `Ativos`: jobs com status `queued`, `running` ou `cancelling`.
  - `Jobs`: histórico completo de execuções de treinamento e autotracking.
  - `Sistema`: telemetria de infraestrutura e alertas de orquestradores/nós.
- **Funcionalidades e Ações Rápidas:**
  - Campo de busca em tempo real (`SearchInput`) filtrando por ID, modelo ou dataset.
  - Telemetria de nós em tempo real (CPU, RAM, VRAM, GPUs).
  - Cancelar / Abortar job ativo diretamente com diálogo de confirmação (`ConfirmDialog`).
  - Download rápido de artefatos de treinamento gerados (`best.pt`, `last.pt`, `boxes.json`, `metrics.jsonl`).
  - **Aplicar boxes do AutoTracker:** botão contextual para aplicar anotações detectadas diretamente ao dataset, com opção de sobrescrever (`overwrite`).
- **Disparo global:** acionado pelo botão no Header, na Sidebar ou via evento customizado de janela (`lib/events.ts::openActionCenter()` disparando `ACTION_CENTER_EVENT`).

### 4.4 Elementos transversais e biblioteca atômica (`components/ui/`)

- Biblioteca atômica dedicada em `components/ui/` (20 primitivas de UI unificadas sob o design system Arcane v2.1; ver especificação completa em `apps/web/components/ui/README.md`):
  - `Button`, `GlassCard`, `Badge`, `Input`, `SearchInput`, `Select`, `SegmentedControl`, `SubmodulePills`, `Modal`, `Drawer`, `Slider`, `ProgressBar`, `MetricTile`, `StatCard`, `Breadcrumbs`, `TruncatedText`, `EmptyState`, `ConfirmDialog`, `Toast`, `DropOverlay`, `ZoomControl`, `Kbd`.
- Modais de sistema em `components/studio/`: `CreateDatasetModal`, `ClassesModal`, `ImportDatasetModal`, `TrainYoloModal`, `AutoTrackerModal`, `ConfirmDialog`.
- Sistema global de toasts (`ToastHost` e `showToast(message, type)` com suporte a ações interativas de auto-dismiss).

## 5. Datasets + Galeria + Editor (coração da IDEIA)

### 5.1 Painel / Dashboard (`app/(studio)/dashboard/page.tsx`)

- **Landing page principal do estúdio:** rota `/` redireciona automaticamente para `/dashboard`.
- **Cabeçalho com identidade:** "Operador local" derivado da presença do cookie válido (`GET /api/auth/me` 200 — ADR-0009 D6); versão de produto via `GET /health` (`version: "0.1.0"`, ADR-0009 D5).
- **KPIs de Alto Nível (`StatCard`):** 4 cartões em `.glass-card` exibindo Datasets totais, Modelos & Pesos (`GET /api/models` — checkpoints de treinos done, sem dedupe; subtexto "pesos de treinos"), Storage (`GET /api/storage/usage` — `datasetsBytes + artifactsBytes + modelsBytes`), Jobs ativos/concluídos (`GET /api/jobs`).
- **Visão do Cluster de Nós:** monitoramento a partir de `GET /api/orchestrators` — nó real `orchestrator-local` (sem RunPod); telemetria global via `GET /api/telemetry` (CPU/RAM reais, `measured:true` com `gpus:[]`/`vram_*:null` no mock — R5); card GPU mostra nome da placa via `gpus[0]`, VRAM = GB+pct (quando disponível). **Emenda G.7 (ADR-0010):** durante a sessão GPU (TrueNAS, `items.length===1`), o dashboard mostra **gauges reais** do nó remoto (nome da GPU real, VRAM/RAM do TrueNAS) — regra ADR-0009 D1 cumprida pelo contrato operacional. Após o teardown, volta a "sem GPU (mock)" (estado restaurado honesto).
- **Controles de visualização:** polling a cada 3s com pausa automática quando a aba perde foco (`visibilitychange`).

### 5.2 Lista de Datasets (`datasets-workspace`)

- Header "Gerenciador de Datasets", toggle grade/lista (`viewMode`) e botão "Novo Dataset".
- **Ingestão Unificada por Drag & Drop (`DropOverlay` + `useFileDrop`):** soltar arquivos `.zip` ou pastas em qualquer ponto da tela aciona a pré-inspeção imediata (`lib/dataset-inspector.ts`) e abre o `CreateDatasetModal` em modo importação pré-configurado.
- **Modal de Criação / Importação (`CreateDatasetModal.tsx`):**
  - Modo Criação Vazia: nome do dataset com slug preview automático em kebab-case, seleção de tipo/tarefa (`TYPE_OPTIONS`), definição de classes CSV.
  - Modo Importação / Inspeção: detecção automática de formato de dataset (YOLO, Difusão, CLIP), contagem de imagens e anotações inspecionadas, e ingestão direta.
  - **Upload em lote (Fatia R1):** envio fatiado em lotes de ≤24 MiB E ≤20 arquivos; arquivo >200 MiB vira lote próprio; sequencial com 1 lote em voo. Barra de progresso "Enviando N de M — lote i de j" + botão "Cancelar envio" (cancelamento entre lotes com retorno parcial). Aviso inline prévio de oversized antes de iniciar. Toast final com contagem completa (importadas/rejeitadas/cancelamento). Rodapé do modal mostra `busyText` durante o upload.
- Filtros por formato/tarefa: busca debounced (`SearchInput`) + pills `Todos / Difusão / OpenCLIP / YOLO` com contadores mono.
- Cards modulares de dataset (`DatasetCard.tsx`) e visualização em tabela (`DatasetTable.tsx`).
- Menu contextual de ações rápidas (`DatasetMenu.tsx`): abrir galeria, treinar dataset, executar AutoTracker, exportar backup ou excluir.

### 5.3 Galeria de Imagens (`dataset-gallery`)

- Breadcrumb de navegação de volta, título, metadados (`type`, contagem de imagens, tamanho formatado e origem storage).
- **Upload por Drag & Drop em lote:** soltar imagens ou vídeos na galeria ativa `DropOverlay` e executa o envio em lote com indicador de progresso e toast de retorno. **Chunking client-side (Fatia R1):** lotes de ≤24 MiB/≤20 arquivos, sequenciais com 1 em voo; arquivo >200 MiB vira lote próprio; tile "Adicionar imagens" virou `div role="button"` com botão "Cancelar" funcional durante o upload; resumo completo com `stored/rejected/failed` por item (contrato §10 upload 3b INTACTO — chunking é client-side).
- **Cards Modulares de Mídia (`ImageCard.tsx`):**
  - Exibição de thumbnail com aspect ratio preservado.
  - Overlay de caixas delimitadoras (BBoxes) para datasets YOLO ou indicador de legendas (`caption`) para Difusão/CLIP.
  - Ações rápidas no hover: exclusão suave (mover para lixeira com toast e ação Desfazer), busca de imagens similares via OpenCLIP.
- **Paginação Contínua (Infinite Load):** paginação em blocos de 50 itens (`PAGE_LIMIT = 50`) com indicador de carregamento dinâmico (`loadingMore`).
- **Ações Principais:**
  - Exportar / Importar backup estruturado (`.zip + dataset.yaml + anotações + captions.jsonl`).
  - AutoTracker modal (`AutoTrackerModal.tsx`): disponível para datasets YOLO com classes e imagens, disparando o job de detecção automática para posterior aplicação.
  - AutoLabel modal (`AutoLabelModal.tsx`): disponível para datasets com imagens, permitindo legendagem em lote com modelos VLM (Florence-2, Qwen2-VL, OpenAI API Vision ou Mock local), com presets direcionados para Difusão LoRA, suporte a endpoints compatíveis (Ollama/vLLM) e seleção de nó de execução.
  - Treinar este Dataset: abre o modal de configuração de treinamento YOLO (`TrainYoloModal.tsx`).
  - Gestão de classes (`ClassesModal.tsx`) e lixeira restaurável (`softDeleteImage`, `restoreImage`, `purgeTrash`).
  - Painel de busca semântica integrada com OpenCLIP (`searchDataset` e `searchByImage`).

### 5.4 Editor BBox (`gallery-bbox-editor`)

- Sidebar 288px: voltar, ferramentas (`bbox B`, `select V`, `pan H`), classes do dataset (`solda_fria` emerald, `curto_circuito` amber, `componente_ausente` rose, `trilha_rompida` cyan + atalho `[1-4]`), painel "Coordenadas YOLO (Norm.)" X/Y/W/H, "Salvar Anotações".
- Canvas central com toolbar flutuante (zoom 50–250%, Reset 100%), moldura 600×450 escalada por `canvasZoom`, caixas selecionáveis com anel `ring-white/50` + alça `se-resize`.
- No real: implementar drag/resize de verdade, snap, validação `0≤x,y,w,h≤1`, atalhos B/V/H/Delete, autosave debounced → `PUT /api/datasets/:id/images/:imageId/boxes`.
- Gerenciar classes no editor — IMPLEMENTADO (Fatia 3g): botão "Gerenciar classes" abre o mesmo `ClassesModal` da galeria (copy nova: renomear preserva caixas; remover classe com caixas é bloqueado com 409); o editor NÃO tem exclusão de imagem (um lugar só destrói: a galeria).

## 6. Módulos de treino (1 aba por categoria)

### 6.1 YOLO (`yolo-workspace`) — preparo via AutoTracker

- Esquerda: backbone dropdown em vidro (`yolo11n.pt`, `yolo11m.pt` recomendado, `yolo11x.pt`, `yolov9-c.pt`, `yolo11-seg.pt`), dataset vinculado (só `category==='yolo`), epochs/batch (`8/16/32/64` com hints de GPU)/imgSize (`416/640/1024`)/otimizador (`AdamW/SGD/Muon`), slider `lr0` 0.0001–0.01, checks Mosaic/Mixup+Flip/Anchor-Opt, CTA Iniciar/Pausar/Abortar.
- Direita: banner status com `training-pulse` + métricas `box_loss / mAP@50 / mAP@50-95` + barra %, 2 gráficos SVG (Loss Treino×Val, mAP), grid "Inferência de Validação" (3 amostras clicáveis com bbox + `Threshold > 0.65`), terminal (`Console de Telemetria`, `Stream WebSocket Ativo`, 36 linhas, `slice(-50)`).
- Mock loop: `setInterval 3000ms` incrementa epoch, deriva loss/mAP, appenda log `[Epoch N/M] box_loss... dfl_loss... mAP50...`. Substituir por job real + WS.

### 6.2 Difusão LoRA (`app/(studio)/difusao/page.tsx` + `ForjaDifusaoSetup.tsx`)

- **Página de Forja Difusão LoRA (`/difusao`):**
  - Rota ativada na sidebar com badge `Diffusers Engine` e link rápido para a página de execuções (`/jobs`).
  - Formulário centralizado de setup de treinamento (`ForjaDifusaoSetup.tsx`):
    - **Dataset Selector:** listagem de datasets elegíveis (com imagens), exibindo badges com contagem de imagens e categoria (`difusao`).
    - **Seletor de Modelo Base:** cards selecionáveis com indicação preditiva de VRAM: `SDXL 1.0` (~12 GB), `Flux.1-dev` (~16 GB), `Stable Diffusion 1.5` (~8 GB).
    - **Trigger Word:** input de palavra-chave ou token identificador adicionado ao início de cada legenda de imagem durante o empacotamento.
    - **Hiperparâmetros LoRA:** épocas (1–100), batch size (1, 2, 4, 8), dimensão do rank LoRA (4, 8, 16, 32, 64, 128) e taxa de aprendizado.
    - **Pesos Iniciais:** seletor opcional para fine-tune a partir de um adaptador LoRA existente no catálogo de modelos (`engine === 'diffusion'`).
    - **Nó de Execução:** seletor `<NodeSelect />` para direcionamento explícito do treinamento a um nó/orquestrador.
    - **Estimador Preditivo de VRAM & Alerta Anti-OOM:** monitor de VRAM estimada comparada com a telemetria em tempo real do nó com botão de auto-correção para perfil seguro.
    - **Submissão:** chamada à API via `startDiffusionJob(params)` com redirecionamento para `/jobs?job={id}`.

### 6.3 OpenCLIP (`openclip-workspace`) — preparo via AutoLabel

- Esquerda: backbone (`ViT-B-32 laion2b`, `ViT-L-14 openai`, `ViT-H-14 laion2b`, `xlm-RoBERTa-large + ViT-H-14`), dataset (só `openclip`), embed dim (`512/768/1024`), loss (`InfoNCE/SigLIP`), slider LR com warmup, batch contrastivo grande (`64/128/256/512` com hints), epochs, CTA.
- Direita: `Contrastive Loss 2.184`, `Recall@1 78.2%`, `Recall@10 94.6%`, curva Recall SVG, sandbox busca semântica (`rotulo de embalagem adulterado`, top-k 8, "Buscar").

## 7. Ferramentas de preparo (abas próprias)

Padrão comum: **config à esquerda + sandbox de teste à direita + "Executar no Dataset" em lote**. Sandbox sempre antes do lote.

### 7.1 AutoTracker (`autotracker-workspace`) → alimenta YOLO

- Entrada: Imagens (lote) | Vídeo (frames + tracking). Execução: Modelo Local (GPU Docker) | Upload + GPU Remota (VPS/RunPod, aceita `.pt/.safetensors/.onnx`, "orquestrador sobe o serviço lá").
- Modelo local: `florence-2-large` (open-set), `yolov8x-world` (zero-shot), `qwen2-vl-7b` (4-bit GPTQ). Prompt textarea + "Predefinido PCB". Slider confidence 0.3–0.95 (default 0.65), check sobrescrever, dataset alvo YOLO.
- Sandbox: seletor PCB/Drone, "Testar Inferência" com `scan-laser` 1.6s, preview com 2 caixas + exemplo `YOLO TXT` (`0 0.485120 0.521800 ... # solda_fria (conf: 0.96)`).

### 7.2 AutoLabel (`autolabel-workspace`) → alimenta Difusão + OpenCLIP

- Execução: Modelo Local | API/Remoto (formato OpenAI). Local: `florence-2-large` (dense caption), `qwen2-vl-7b`, `blip-2` (LoRA). API: `gpt-4o-vision`, endpoint compatível (vLLM próprio), `claude-3-5-sonnet`. Sempre + botão "Enviar modelo personalizado".
- Prompt caption + "Predefinido Difusão", dataset alvo (só `difusao/openclip`).
- Sandbox: amostras Retrato (Difusão)/Embalagem (CLIP), "Testar Caption", preview `.txt/.json` + exemplo `captions.jsonl {"image": "img_0042.jpg", "caption": "..."}` e `datasets/.../labels/img_0042.txt`. Backend Rust persiste junto ao dataset.

## 8. Modelos de dados (proposta TS para o Next.js)

```ts
type DatasetCategory = 'difusao' | 'openclip' | 'yolo';
type DatasetStatus = 'ready' | 'in_progress' | 'needs_labeling';

interface Dataset {
  id: string; title: string; // slug kebab-case
  category: DatasetCategory; type: string; task: string;
  imagesCount: number; labeledCount: number; classes: {id: string; name: string; idx: number; color: string}[]; // objeto desde a 3b.7 (gap do classId fechado: PUT boxes usa classes[].id como classId)
  format: string; status: DatasetStatus; lastModified: string;
  size: string; autoTracked: boolean; source: string;
  trashCount: number; // lixeira restaurável desde a 3g (ADR-0005)
}
// ALINHADO ao contrato real (Fatia 3a — packages/contracts/openapi.yaml, ADR-0002;
// wire camelCase global): slug entra no shape (gerado pelo servidor, UNIQUE);
// size → o servidor devolve sizeBytes: number e a UI formata em lib/format.ts;
// lastModified = RFC 3339 (datasets.updated_at), não string relativa;
// type no wire é um dos 4 códigos de máquina (yolo_bbox|yolo_seg|difusao_lora|clip_image_text),
// o rótulo pt-BR é da UI; source é derivado (null em vazio, `s3://{bucket}/datasets/{id}/`
// com imagens — ADR-0003 D5); classes é objeto {id,name,idx,color} desde a 3b.7;
// autoTracked deriva do banco desde a 3d: EXISTS sobre `boxes.origin='autotracker'`
// (dívida T7 do ADR-0002 quitada — não é mais constante).
// trashCount deriva do banco desde a 3g (ADR-0005): COUNT de images com deleted_at
// NOT NULL (badge da pill Lixeira; refreshDataset relê via getDataset).
interface BBox { id: number; classId: number; label: string; x: number; y: number; w: number; h: number; color: string; }
// trainTabFor(ds): difusao→/difusao, openclip→/openclip, yolo→/yolo
// selectDatasetValue(cats): filtra por categoria, fallback 1º compatível
```

## 9. Fluxos principais

1. Criar → galeria vazia → Enviar amostras → AutoLabel/AutoTracker → % rotuladas sobe → Treinar.
2. Treino YOLO: vincular dataset YOLO → hiperparams → Iniciar → monitor (epoch/loss/mAP/logs/val) → Pausar/Abortar → pesos exportados.
3. Preparo seguro: ajustar prompt/modelo/threshold no sandbox → Testar Inferência/Caption → Executar no Dataset (lote).
4. Correção manual: validação ou galeria → editor BBox → Salvar → re-treino.
5. Backup — IMPLEMENTADO (Fatia 3e): Exportar (`.zip + dataset.yaml + anotações + captions.jsonl`) / Importar (mesmo pacote) na galeria, com diálogo de substituição no 409.

## 10. Contratos que o front vai exigir do Rust (alinhado com backend.md §9)

- Auth (IMPLEMENTADO Fatia 2 — `app/login/page.tsx`, `proxy.ts`, `next.config.ts`; contrato `packages/contracts/openapi.yaml`, `docs/adr/0001-auth-single-user.md`): `POST /api/auth/login`, `GET /api/auth/me`, `POST /api/auth/logout` + `GET /health` (`{status, service, auth: ready|setup_required, version}`). A rota `/health` é pública e devolve `version` (ADR-0009 D5; spec 0.9.0) — a fonte real da versão de produto (não "v1.3.0" hardcoded).
  - Rota `/login`: form de senha; erros ramificados por `code` em pt-BR (`invalid_credentials` → "Senha incorreta.", `setup_required` → "Servidor em modo setup — defina STUDIO_PASSWORD.", `invalid_request` → "Envie a senha.", default → "Falha inesperada."); sucesso → `/` (`router.replace` + `refresh`); já logado (`GET /me` ok) → volta a `/`.
  - Gate de sessão via `proxy.ts`: `/login` passa direto (decide por si via `/me`); sem cookie `heph_session` → redirect `/login`; com cookie → passa, validade decidida pelo servidor via `/me` (`/` redireciona a `/login` se `/me` não-ok; logout → `POST /logout` + volta a `/login`). `/api/*` fora do matcher — envelope 401 do backend repassado intacto.
  - Resolução T5: front chama `/api/*` relativo (`credentials: "same-origin"`, sem CORS); rewrite Next → `API_INTERNAL_URL` (dev `http://localhost:8080`, compose `http://principal:8080`). `NEXT_PUBLIC_API_URL` ficou como resíduo de build (só `ARG` no Dockerfile; runtime usa o proxy `/api`).
- Datasets: `GET/POST /api/datasets`, `GET/DELETE /api/datasets/:id` — IMPLEMENTADO (Fatia 3a — contrato `packages/contracts/openapi.yaml`, ADR-0002). Upload/imagens/boxes/caption — IMPLEMENTADO (Fatia 3b — spec 0.3.0, contrato que a 3c/3d implementa; as rotas de UI que os consomem ainda NÃO existem — `/datasets` é a 3c): `POST /:id/upload` (multipart `files`; corpo total 200 MiB + 8 MiB envelope → 413; teto por arquivo 200 MiB → item `rejected/too_large`; resposta `{items:[{imageId,filename,status,reason,bytes,width,height}]}` camelCase). **Upload em lote client-side (Fatia R1):** `lib/images.ts::uploadImages` fatia internamente em lotes ≤24 MiB/≤20 arquivos (arquivo >200 MiB = lote próprio; sequencial, 1 em voo); pré-validação sem rede de oversized; erro de rede/500 → `status:"failed"` `reason:"storage_error"`; break por 413 → arquivos restantes `failed`/`envelope_limit` + resumo completo; cancelamento entre lotes com retorno parcial. Contrato da rota `POST /:id/upload` NÃO mudou — chunking é transparente ao backend. `GET /:id/images?limit(=50, máx 200)&offset(=0)&split(train|val)&labeled(bool)` → `{items,total,limit,offset}`, `GET /:id/images/:imageId` (Image flat + `boxes[]` + `caption|null`), `GET /:id/images/:imageId/data` (proxy incondicional, `Cache-Control: private, max-age=31536000, immutable`), `PUT .../images/:imageId/boxes` (`{boxes:[{classId,x,y,w,h,conf?,origin?,trackId?}]}` cap 1000, domínio 0..1), `PUT .../images/:imageId/caption` (upsert `{text:1..8000,origin?,model?≤255}`).   Export/import — IMPLEMENTADO (Fatia 3e, spec 0.6.0, ADR-0006): `POST /:id/export` (download `.zip` via blob + `Content-Disposition`), `POST /datasets/import` (FormData `file` + `title` opcional + `replace: "true"` só quando true; fluxo 409 → diálogo de irreversibilidade → `replace=true`); `lib/backup.ts` (`exportDataset`/`importDataset` + copies por `code`) + `components/studio/ImportDatasetModal.tsx`; limite do zip 200 MiB + 8 MiB envelope (413). `POST /:id/package` — IMPLEMENTADO (Fatia 4, spec 0.7.0, ADR-0007 D1): congela `dataset_versions`, gera zip, PUT `packages/<version_id>/`, 200 `PackageResponse{versionId,key,bytes,md5Zip,files}`.
- Ambientes (alias UI de orquestradores): o alias `/api/environments*` **implementado** (Fatia H, ADR-0011 D5/D7 — módulo "Orquestradores" habilitado, badge Roadmap removido). `GET /api/environments` ≡ `GET /api/orchestrators` (enriquecido com telemetria por nó); `POST /api/environments/adopt` e `POST /api/environments/:id/revoke` ≡ handlers de orchestrators (mesmas respostas). Página `/environments` com lista, status chips, gauges por nó, modal Adotar com `pairingCode`, Revogar com ConfirmDialog, empty state.
- Jobs — IMPLEMENTADO (Fatia 4 e 5; spec 0.7.0 e 0.8.0, ADR-0007 e ADR-0008):
  - Rota `/treino` (Treino YOLO — nova, F6.3): setup de treino YOLO centralizado (`ForjaYoloSetup.tsx` movido para lá); pós-202 navega `/jobs?job=<id>` para auto-seleção na fila de execuções. Sidebar: "Treino YOLO" aponta para `/treino`.
  - Rota `/jobs` (Execuções — renomeada F6.3): fila de trabalho de todos os tipos + histórico agrupado ativos primeiro + painel de detalhe; CTA "Novo Treino" primário; badge jobsActive na Sidebar.
  - Sidebar: seção "Treinamento & Execução" com "Treino YOLO"→/treino (sem badge) e "Execuções"→/jobs (badge jobsActive); breadcrumbs `treino`→"Treino YOLO", `jobs`→"Execuções".
    - Setup de treino YOLO (`ForjaYoloSetup.tsx`): formulário controlado com validação para dataset YOLO, modelo backbone (`yolo11n.pt`, etc.), epochs, batch size, imgsz, taxa de aprendizado lr0, otimizador (`AdamW/SGD/Muon`) e data augmentations.
    - Gráficos vetoriais de convergência (`ConvergenceChart.tsx` e `MetricSparkline`): exibição em SVG de Loss (box, cls, dfl) e mAP (mAP50, mAP50-95) sincronizados via polling em tempo real.
    - Terminal estruturado de telemetria e logs (`JobLogViewer.tsx`): visualizador em `JetBrains Mono` com busca textual, auto-scroll e filtro de severidade de logs de streaming.
    - Botão contextual **"Aplicar boxes ao dataset"**: habilitado quando `status==='done' && engine==='autotracker'` e o artefato `boxes.json` existe, com checkbox de `overwrite` opcional.
  - `POST /api/jobs/yolo` (`lib/jobs.ts:startYoloJob`): body `{datasetId,model,epochs,batch,imgsz,lr0,optimizer,augment}`, 202 `{jobId,status:"queued",queuePosition?}`.
  - `GET /api/jobs` (`lib/jobs.ts:listJobs`): response camelCase `{items:[Job],total}` (JobList). Polling 3s ativo quando há jobs `queued`/`running`/`cancelling`; pausa quando todos terminam (`jobs/page.tsx`).
  - `GET /api/jobs/:id/metrics` (`lib/jobs.ts:getJobMetrics`): `{items:[{epoch,boxLoss,clsLoss,dflLoss,map50,map5095}]}` (camelCase wire; `mAP50-95` → `map5095`). Série por epoch.
  - `GET /api/jobs/:id/artifacts` (`lib/jobs.ts:getJobArtifacts`): `{items:[{id,kind,path,md5,bytes}]}`.
  - `GET /api/jobs/:id/artifacts/:artifactId/data` (`lib/jobs.ts:downloadArtifact`): download via blob + `document.createElement("a")` + `URL.createObjectURL`.
  - `POST /api/jobs/:id/abort` (`lib/jobs.ts:abortJob`): 200 `{"status":"cancelling"|"cancelled"}` ou 409 `job_not_abortable`. UI: `ConfirmDialog` antes de abortar.
  - `DELETE /api/jobs/:id` (`lib/jobs.ts:deleteJob`): 200 `{id,status,artifacts,objectKeys,modelsDeleted,generationsPreserved}` (wire camelCase) | 404 `not_found` | 409 `job_not_terminal` | 503 `queue_unavailable`. Tela `/jobs`: botão "Excluir" visível só em jobs terminais (`done`/`failed`/`cancelled`), com `ConfirmDialog` de confirmação destrutiva; sucesso → toast com a contagem de modelos removidos (`modelsDeleted`) e gerações preservadas (`generationsPreserved`) + refresh da lista.
  - `POST /api/jobs/cleanup` (`lib/jobs.ts:cleanupJobs`): body `{olderThanDays?, statuses?}` (ambos opcionais) → 200 `{deleted,jobs,objectKeys}` | 400 `invalid_request` | 503 `queue_unavailable`. Tela `/jobs`: botão "Limpar antigos" abre modal com idade (7/30/90 dias envia `olderThanDays`; "Todos os jobs terminais" envia `statuses: [done,failed,cancelled]`, nunca body vazio); sucesso → toast com `deleted` + refresh da lista.
  - AutoTracker — IMPLEMENTADO (Fatia 5/9, ADR-0008, ADR-0014, spec 0.13.0):
    - `POST /api/jobs/autotracker` (`lib/autotracker.ts:startAutotrackerJob`): body `{datasetId, model?, conf?, modelId?}`, 202 `{jobId,status:"queued",queuePosition?}`. Modal `AutoTrackerModal` — dropdown "Modelo": Mock determinístico (default, sem modelId) + modelos `engine=world` via `listModels()` filtrado client-side; empty state honesto "Importe pesos world em Modelos & Pesos"; erros 404 com modelId → "Modelo não encontrado — atualize a lista"; 404 sem modelId → "Recurso não encontrado.".
    - `POST /api/jobs/:id/autotracker/apply` (`lib/autotracker.ts:applyAutotrackerBoxes`): body `{overwrite?, imageId?}`, 200 `{applied, skipped, images}`; 409 `job_not_done` (job não está `done`); 400 `invalid_request`; 404 `not_found`; 503 `queue_unavailable`/`storage_unavailable`.
    - `autotrackerErrorMessage` (`types/studio.ts`): compartilhado entre AutoTrackerModal, ActionCenter e /jobs; mantém "Job não encontrado." para o apply (actions em /jobs e ActionCenter).
  - AutoLabel — IMPLEMENTADO (Fatia AutoLabel v1, ADR-0016, spec 0.15.0):
    - `POST /api/jobs/autolabel` (`lib/autolabel.ts:startAutolabelJob`): body `{datasetId, model?, prompt?, orchestratorId?}`, 202 `{jobId,status:"queued",queuePosition?}`. Modal `AutoLabelModal` — disparado a partir da Galeria do Dataset quando `imagesCount > 0`. Campo modelo fixo mock ("Mock (determinístico)"), input de prompt livre opcional, seleção de nó com `NodeSelect`.
    - `POST /api/jobs/:id/autolabel/apply` (`lib/autolabel.ts:applyAutolabelCaptions`): body `{datasetId?, overwrite?}`, 200 `{applied, skipped, images}`; 409 `job_not_done` (job não está `done`); 400 `invalid_request`; 404 `not_found`; 503 `queue_unavailable`/`storage_unavailable`. Botão contextual "Aplicar legendas ao dataset" em `/jobs` e no ActionCenter com checkbox de overwrite.
    - `autolabelErrorMessage` (`types/studio.ts`): mensagens em pt-BR padronizadas.
  - **Centro de Atividades (`ActionCenter.tsx`):** consome `/api/jobs` e `/api/telemetry` em tempo real em uma gaveta global (`Drawer`), permitindo abortar jobs, baixar artefatos e aplicar anotações do AutoTracker sem sair do contexto de trabalho atual.
  - Telemetria real (polling `getTelemetry()` a cada 3s via `Sidebar.tsx`, `ActionCenter.tsx` e `dashboard/page.tsx`): `Telemetry{measured,cpu,ram,ramTotal,vramUsed,vramTotal,gpus,jobsActive}`. `ramTotal` (bytes, aditivo) = total de RAM do nó; a UI calcula `ramGB = ram / (1024**3)` e `ramPct = ram / ramTotal`. **Emenda H.7 (ADR-0011):** com >1 nó, `cpu/ram/ramTotal` = `null` (não agregáveis); `gpus` = união; `vramUsed/vramTotal` = soma; `measured:true` se ≥1 nó fresco. Dashboard compõe gauges por nó a partir de `/api/orchestrators` enriquecido (regra `items.length===1` morta — ADR-0011 D7).
  - **Monitoring — IMPLEMENTADO (F6.1 + Fatia H; ADR-0009 + ADR-0011):**
    - `GET /api/orchestrators` (`lib/monitoring.ts:listOrchestrators()`): `Orchestrator{id,name,kind,endpoint,status,lastHeartbeat,measured,cpu,ram,ramTotal,vramUsed,vramTotal,vramTotalGb,gpus,jobsActive}` — consome o nó real da tabela do manager, **enriquecido com telemetria por nó** (Fatia H). 503 `queue_unavailable` quando manager fora (UI mostra "Indisponível (manager fora)", não vazio).
    - `POST /api/environments/adopt` (`lib/monitoring.ts:adoptOrchestrator(body)`): body `{name, endpoint, kind, pairingCode}` → 200 (upsert) | 400 `invalid_request` | 409 `pairing_invalid` | 503. Erros mapeados em pt-BR ("Código de pareamento inválido ou orquestrador inalcançável" / "Indisponível (manager fora)").
    - `POST /api/environments/:id/revoke` (`lib/monitoring.ts:revokeOrchestrator(id)`): 204 | 404 `not_found` | 503.
    - `GET /api/models` (`lib/monitoring.ts:listModels()`): `Model{id,name,engine,model?,source,bytes,md5,url?,jobId?,createdAt,kind?,arch?}` — tabela canônica `models` (Fatia I; ADR-0012 D2/D6; `kind`/`arch` só p/ diffusion — ADR-0023 D4). 503 `queue_unavailable` quando manager fora.
    - `GET /api/storage/usage` (`lib/monitoring.ts:getStorageUsage()`): `StorageUsage{datasetsBytes,artifactsBytes,modelsBytes,totalBytes,measured:true}` — soma SQL por dono; `artifactsBytes` exclui `kind='model'` (Fatia I; ADR-0012 D8). 503 `queue_unavailable` quando manager/database fora.
    - `GET /health` (`lib/monitoring.ts:getHealth()`): `HealthResponse{status,service,auth,version}` — versão de produto real (`CARGO_PKG_VERSION`, não string fixa).
- Runners/playground: `POST /runners/{engine}/up`, `POST /runners/:id/{kill,infer}`, `GET /runners` — infer via `POST /:id/infer`, 409 se preemptado.
- Models — IMPLEMENTADO (Fatia I; ADR-0012; Gestão de Modelos 2026-09-12):
  - `GET /api/models` (`lib/models.ts:listModels()`): `Model{id,name,engine,model?,source,bytes,md5,url?,jobId?,createdAt,kind?,arch?}` — tabela canônica `models` (`kind`/`arch` só p/ diffusion — ADR-0023 D4). 503 `queue_unavailable` quando manager fora.
  - `POST /api/models/upload` (`lib/models.ts:uploadModel()`): multipart `file`+`engine`+`name?` → 201 `Model`. Suporta engines `yolo`, `world`, `diffusion`, `clip` e formatos `.pt` e `.safetensors`. Erros: 400 `invalid_request` (engine inválida, ext não permitida, magic divergente), 413 `invalid_request` (envelope), 503 `queue_unavailable`. UI: `ModelUploadModal.tsx`.
  - `POST /api/models/download` (`lib/models.ts:downloadModel()`): `{url, engine, name?}` → 201 `Model`. Suporta engines `yolo`, `world`, `diffusion`, `clip` e extensões `.pt` e `.safetensors`. Erros: 400 `invalid_request` (scheme/host negado), 403 `model_download_disabled` (allow-list ausente/vazia — UI mostra "Download por URL desabilitado — configure MODEL_DOWNLOAD_ALLOWED_HOSTS no .env"), 502 `model_download_failed`, 503 `queue_unavailable`. UI: `ModelDownloadModal.tsx`.
  - `DELETE /api/models/:id` (`lib/models.ts:deleteModel(id)`): 204 No Content, 401, 404. Remove objeto do bucket se source upload/download e desvincula registro no manager. UI com `ConfirmDialog` de confirmação destrutiva.
  - **Página `/models`** (Fatia I.5; ADR-0012 D7; atualizada em Gestão de Modelos): rota `(studio)/models/page.tsx` com lista GlassCards (nome mono, badges de engine YOLO/YOLO-World/Difusão/CLIP, badge de formato .safetensors/.pt, badge origem Treino/Upload/Download, bytes formatados, data, botão Baixar via URL presigned e botão Excluir com `ConfirmDialog`), modais Upload e Baixar por URL suportando `.pt` e `.safetensors`, empty state, erros mapeados em pt-BR por `code`. Sidebar "Modelos & Pesos" habilitado (`isAvailable: true`).
  - **Dropdown de pesos em `/treino`** (Fatia I.6; ADR-0012 D7): `ForjaYoloSetup.tsx` ganha seletor "Pesos iniciais" (`listModels()` filtrado por `engine==='yolo'`, rótulo `name · source`; selecionado → envia `weights: id` no `startYoloJob`). TrainYoloModal da galeria INTOCADO.
  - `POST /api/jobs/yolo` (`lib/jobs.ts:startYoloJob`): body `{datasetId,model,epochs,batch,imgsz,lr0,optimizer,augment,weights?}` — `weights?: string` (UUID de `models`, opcional). Erros: 400 `invalid_request` (weights não-UUID), 404 `not_found` (weights inexistente).
- Playground — IMPLEMENTADO (Fatia J; ADR-0013, Atualizado ADR-0023 D0 — YOLO-only):
  - `POST /api/jobs/predict` (`lib/playground.ts:startPredictJob`): body `{modelId, datasetId, conf?}` (conf default 0.65). 202 `{jobId,status:"queued",queuePosition?}`. Erros: 400 `invalid_request`, 404 `not_found`, 409 `dataset_not_ready`, 503 `queue_unavailable`. UI: toast 202 + link "Acompanhar em Execuções" (`/jobs?job=<id>`).
  - **Página `/playground`** (Fatia J.5; ADR-0013 D7, Atualizado ADR-0023 D0 — YOLO-only): rota `(studio)/playground/page.tsx` — workspace 2 colunas (DESIGN.md). Sem pills de modo; página é exclusivamente Detecção YOLO. Coluna de controle (320–384px): seletor Modelo (`listModels()` filtrado por `engine==='yolo'`, rótulo `name · source · variante`), seletor Dataset (`canPredict` = category yolo + imagesCount>0 — classes NÃO obrigatórias), slider conf 0.3–0.95 default 0.65, CTA "Executar Inferência". Coluna de resultado: lista jobs predict (filtro `mode==='predict'` no `GET /api/jobs`); job done → overlay de boxes sobre imagens (match por filename entre predictions.json baixado via proxy de artefato e lista de imagens — `GET /api/datasets/:id/images`; cores por `cls.color` do dataset, fallback zinc #71717a); stats imagens/com detecção/boxes/skips; download predictions.json (proxy `.../artifacts/:id/data`); job failed visível com CTA /jobs; empty states honestos. Card de job sem botão aninhado.
  - `lib/playground.ts` (`startPredictJob`, `getPredictions`) + `types/studio.ts` (aditivo: `PredictionsData`, `predictErrorMessage`). Erros mapeados em pt-BR por `code` (padrão da casa).
  - Sidebar "Detecção YOLO" habilitado (`isAvailable: true`, badge Roadmap removido, Fatia J; ADR-0013 D7).
  - **Nota:** `POST /runners/{engine}/up`, `POST /runners/:id/{kill,infer}`, `GET /runners` continuam dívida (runners quentes — ADR-0013 D0). O v1 usa job assíncrono na fila + página `/playground` com overlay.
- Preview/sandbox: `POST /api/preview/{autolabel|autotracker|generate|search}` (efêmero, sem fila).
- Geração — IMPLEMENTADO (Fatia Geração; ADR-0023):
  - Rota `/geracao` (`app/(studio)/geracao/page.tsx`): pills internas **Gerar | Galeria** (SubmodulePills). Aba Gerar: painel de geração com modelo base (pills FLUX.2 Klein 4B / SDXL / SD 1.5 / Custom), variante destilada, quantização, editor multi-LoRA (`LoRAEditor.tsx`), prompt, negative, resolução, steps, CFG, seed, batchSize, NodeSelect, CTA "Gerar". Aba Galeria: grade de thumbs com seleção, paginação infinita, delete/export em lote, comparador 2 imagens (`CompareSlider.tsx`).
  - `POST /api/jobs/diffusion/generate` (`lib/playground.ts:startDiffusionGenerateJob`): body `{baseModel?, customModelId?, prompt, negativePrompt?, width?, height?, steps?, guidanceScale?, seed?, quantization?, distilled?, batchSize?, loras?, orchestratorId?}`. `baseModel` e `customModelId` XOR. `loras`: array ≤4 de `{modelId, scale}`. 202 `{jobId,status:"queued",queuePosition?}`. Erros: 400 `invalid_request`, 400 `unsupported_architecture`, 404 `not_found`, 503 `queue_unavailable`. `diffusionGenerateErrorMessage` (`types/studio.ts`): mensagens em pt-BR.
  - `GET /api/generations` (`lib/generations.ts:listGenerations`): `GenerationList{items: Generation[], total}`. Parâmetros: `limit`, `offset`, `baseModel`, `quantization`, `deleted`. Paginação infinita na galeria.
  - `GET /api/generations/:id/data` (`lib/generations.ts:getGenerationDataUrl`): proxy da imagem.
  - `POST /api/generations/delete` (`lib/generations.ts:deleteGenerations`): body `{ids: string[]}` (≤100). 204.
  - `POST /api/generations/export` (`lib/generations.ts:exportGenerations`): body `{ids: string[]}` (≤100). 200 `application/zip` stream.
  - Componentes: `GenerationPanel.tsx` (painel de controle + resultado), `GenerationGallery.tsx` (grade com seleção/delete/export/comparador), `LoRAEditor.tsx` (editor de linhas multi-LoRA), `CompareSlider.tsx` (modal comparador 2 imagens com clip-path reveal).
  - Sidebar "Geração" habilitado (`isAvailable: true`, ícone `IconSparkles`, rota `/geracao`).
  - **Nota:** `/playground` perdeu as pills de modo Difusão e o componente `PlaygroundDiffusion`; agora é exclusivamente Detecção YOLO.
- Settings: chaves `hfToken, civitaiKey, openaiKey, anthropicKey, vllmEndpoint` no wire (camelCase global, ADR-0002 D1; colunas `settings` seguem snake_case) — rota ainda **não implementada** (mascaradas no GET quando chegar).
- Classes e lixeira — IMPLEMENTADO (Fatia 3g, spec 0.4.0, ADR-0005): `putClasses(datasetId, classes)` (`lib/classes.ts` → `PUT /:id/classes`, reconciliação por id, 409 `classes_in_use` mantém o modal aberto); `softDeleteImage` (`DELETE /:id/images/:imageId` → 204, sem sweep) / `restoreImage` (`POST .../restore` → 204 sem conflito | 200 `{filename}` com rename `_restaurado`) / `purgeTrash` (`DELETE /:id/trash` → 204) (`lib/images.ts`; listagem da lixeira via `listImages(id, {deleted:true})`); `Toast.action` (`{label, onClick}`, toast com ação vive 6s — usado pelo Desfazer).
- Busca semântica — IMPLEMENTADO (Fatia 3f, spec 0.5.0, ADR-0004): `searchDataset(datasetId, q, {k?, classId?, split?})` (`lib/search.ts` → `GET /:id/search?q&k&classId&split`, 200 `SearchResponse` | 400 inválida | 409 `index_not_ready` | 503 `embedding_unavailable`), `searchByImage(datasetId, imageId, {k?, threshold?})` (`lib/search.ts` → `POST /:id/search/by-image {imageId,k?,threshold?}`, 200 `SearchResponse` | 400 corpo inválido | 404 imagem fora do dataset | 409 `index_not_ready`), `getSearchStatus(datasetId)` (`lib/search.ts` → `GET /:id/search/status`, 200 `SearchStatus{status,imagesCount,indexedCount,model,dim}`), `triggerSearchIndex(datasetId)` (`lib/search.ts` → `POST /:id/search/index`, 202 `{status:indexing|not_indexed}`) (`types/studio.ts`: `SearchIndexStatus = "not_indexed"|"indexing"|"ready"|"stale"`, `SearchStatus`, `SearchItem{image,score}`, `SearchResponse{items}`); campo da imagem nos resultados é `url` (o mesmo `Image.url` do list — nunca `thumbUrl`).

## 11. Estrutura de pastas do front-end (`apps/web`)

```
apps/web/
├── app/
│   ├── (studio)/
│   │   ├── dashboard/page.tsx            # Painel principal (home pós-login)
│   │   ├── datasets/
│   │   │   ├── page.tsx                  # Lista de datasets (Grade/Lista, Drag & Drop)
│   │   │   └── [id]/
│   │   │       ├── page.tsx              # Galeria de amostras e anotações
│   │   │       └── annotate/[imageId]/
│   │   │           └── page.tsx          # Editor visual de BBox YOLO
│   │   ├── treino/page.tsx               # Setup de treino YOLO (ForjaYoloSetup; F6.3)
│   │   ├── jobs/page.tsx                 # Execuções — fila de trabalho + histórico + detalhe (F6.3)
│   │   ├── geracao/page.tsx              # Geração — pills Gerar|Galeria (ADR-0023)
│   │   ├── playground/page.tsx           # Detecção YOLO — inferência com overlay de boxes (Fatia J)
│   │   └── layout.tsx                    # Shell global (Sidebar + Header + ActionCenter + ToastHost)
│   ├── login/page.tsx                    # Autenticação single-user (AuthAmbient)
│   ├── globals.css                       # Tokens @theme, classes glass e fontes
│   ├── layout.tsx                        # Root layout e fontes locais self-hosted
│   └── page.tsx                          # Redirecionamento para /dashboard
├── components/
│   ├── ui/                               # 20 Primitivas atômicas de UI (Arcane v2.1)
│   │   ├── Button, Badge, Breadcrumbs, ConfirmDialog, Drawer, DropOverlay,
│   │   ├── EmptyState, GlassCard, Input, Kbd, MetricTile, Modal, ProgressBar,
│   │   ├── SearchInput, SegmentedControl, Select, Slider, StatCard, SubmodulePills,
│   │   └── Toast, TruncatedText, ZoomControl, index.ts, README.md
│   ├── studio/                           # Componentes de negócio e workspaces do estúdio
│   │   ├── ActionCenter.tsx              # Gaveta lateral de monitoramento e atalhos
│   │   ├── AutoLabelModal.tsx            # Diálogo de disparo do AutoLabel (Fatia AutoLabel v1)
│   │   ├── AutoTrackerModal.tsx          # Diálogo de disparo do AutoTracker
│   │   ├── ClassesModal.tsx              # Diálogo de gestão de classes do dataset
│   │   ├── CompareSlider.tsx             # Comparador de 2 imagens com clip-path reveal (ADR-0023)
│   │   ├── ConfirmDialog.tsx             # Confirmação de exclusões e aborts
│   │   ├── ConvergenceChart.tsx          # Gráficos de convergência Loss e mAP
│   │   ├── CreateDatasetModal.tsx        # Criação e ingestão com inspeção de pacotes
│   │   ├── DatasetCard.tsx               # Card de dataset em grade com métricas
│   │   ├── DatasetMenu.tsx               # Menu contextual de ações de dataset
│   │   ├── DatasetTable.tsx              # Visão de datasets em lista tabular
│   │   ├── ForjaYoloSetup.tsx            # Painel central de configuração de treino
│   │   ├── GenerationGallery.tsx         # Galeria de gerações com seleção/delete/export/comparador (ADR-0023)
│   │   ├── GenerationPanel.tsx           # Painel de controle de geração (ADR-0023)
│   │   ├── ImageCard.tsx                 # Card modular de imagem com BBoxes/captions
│   │   ├── ImportDatasetModal.tsx        # Diálogo de importação de backup ZIP
│   │   ├── JobCard.tsx                   # Item de histórico e status de job
│   │   ├── JobLogViewer.tsx              # Terminal de logs estruturado com busca
│   │   ├── LoRAEditor.tsx                # Editor multi-LoRA com N linhas (ADR-0023)
│   │   ├── ModelDownloadModal.tsx         # Modal de download de modelo por URL (Fatia I)
│   │   ├── ModelUploadModal.tsx           # Modal de upload de modelo .pt (Fatia I)
│   │   ├── NodeSelect.tsx                 # Seletor de nó de execução de jobs (Fatia N — ADR-0015)
│   │   ├── Sidebar.tsx                   # Barra lateral macro de navegação e telemetria
│   │   ├── Toast.tsx                     # Hospedeiro e disparador de notificações
│   │   ├── TrainYoloModal.tsx            # Modal de treino a partir da galeria
│   │   └── YoloHyperparameters.tsx       # Controles avançados de treino YOLO
│   └── icons.tsx                         # Ícones vetoriais em traço limpo (stroke 1.7)
├── lib/
│   ├── api.ts, autotracker.ts, backup.ts, classes.ts,
│   ├── dataset-inspector.ts              # Pré-inspeção inteligente de pastas/ZIPs
│   ├── datasets.ts, events.ts, format.ts, generations.ts, images.ts, jobs.ts,
│   ├── monitoring.ts                     # Rotas de monitoramento (orchestrators/models/storage/health; F6.1)
│   ├── playground.ts                     # Rotas do Playground + Geração (startPredictJob, getPredictions, startDiffusionGenerateJob; Fatia J + ADR-0023)
│   ├── search.ts
└── types/
    └── studio.ts                         # Tipagens TypeScript de contratos e dados
```

## 12. Backlog front-end (evolução contínua)

- [x] Rotas reais implementadas (`/dashboard`, `/datasets`, `/datasets/[id]`, `/annotate/[imageId]`, `/jobs` (Execuções), `/treino` (Treino YOLO), `/models` (Modelos & Pesos — Fatia I), `/playground` (Detecção YOLO — Fatia J, ADR-0023 D0), `/geracao` (Geração — ADR-0023 D0), `/login`).
- [x] Redirecionamento da raiz (`/` → `/dashboard`).
- [x] Centro de Atividades global (`ActionCenter.tsx`) com gaveta retrátil e monitoramento em tempo real.
- [x] Ingestão unificada por Drag & Drop na tela de datasets com pré-inspeção imediata de `.zip`/pastas (`dataset-inspector.ts`).
- [x] Gráficos de convergência vetoriais reais em SVG (`ConvergenceChart.tsx` e `MetricSparkline`) alimentados por `/metrics`.
- [x] Terminal de telemetria e logs estruturado com busca e auto-scroll (`JobLogViewer.tsx`).
- [x] Biblioteca unificada de 20 componentes atômicos em `components/ui/` sob padrão Arcane v2.1.
- [x] Polling contínuo de telemetria e jobs com otimização via `visibilitychange`.
- [x] Import/export `.zip` estruturado validado (Fatia 3e — galeria + modal + diálogo de substituição 409).
- [x] AutoTracker v1 integrado (Fatia 5 — modal na galeria + apply de boxes em `/jobs` e no `ActionCenter`).
- [x] Visibilidade e seleção manual de nós de execução de jobs (Fatia N — ADR-0015: `NodeSelect` em `/playground`, `/treino` e `AutoTrackerModal`, dados de nó e fallback automático em `/jobs`).
- [ ] Canvas BBox avançado com drag/resize de alta precisão, snap e persistência debounced.
- [ ] Formulários com validação schema completa (Zod) para hiperparâmetros de treino.
- [ ] Testes automatizados de componentes e fluxos E2E com Playwright.

- **Auth — single-user local:** tela `/login` nova (não existe no protótipo). JWT HttpOnly, gate em todas as rotas do studio, logout no menu settings. Sem multi-user por enquanto.
- **Playground (nova aba, mesmo design):** runner sob demanda para os 3 motores — Difusão (gerar imagem), YOLO (inferência imagem/vídeo), CLIP (busca semântica). Orquestrador sobe o runner, mantém ativo até faltar VRAM ou usuário clicar "Matar runner". Card de status com VRAM usada + botão kill + aviso de preempção.
- **Samples por ciclo:** em cada treino (YOLO/Difusão/CLIP) exibir N previews geradas por época/steps com métrica + imagem — é o "health visual". Exige `GET /api/jobs/:id/samples?cycle=N` e grade na direita dos workspaces.
- **Downloads de modelos:** settings com campos HF token + Civitai key (env como fallback) + input de URL. Front só coleta e exibe progresso; download real é do orquestrador.
- **Limites:** upload avulso imagem/vídeo 200 MB por arquivo (validação no front + item `rejected/too_large` e 413 do Rust no corpo total); zip de import: 200 MiB + 8 MiB de envelope (413 do Rust no corpo total; modal mostra "Backup maior que o limite de 200 MiB."). Envio de dataset p/ orquestrador sem limite, com md5 + fragmentação quando remoto — front mostra barra de empacotamento → envio → verificação.
- **Pré-condições 3b/3d (ADR-0002 T3/T4/T7):** 3b ENTREGOU storage+imagens+anotação (upload/imagens/boxes/caption + `source` derivado + sweep de prefixo); export/import/package → 3e. DELETE ganhou sweep de prefixo `datasets/{id}/` pós-commit reapável best-effort (não mais cleanup de `<DATASETS_DIR>/<slug>` — disco morreu, ADR-0003 D7); `jobs.dataset_id ON DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`); derivar `autoTracked` de `boxes.origin='autotracker'` — detalhe no ADR.
- **Fatia redesign UI v2 (branch `feature/redesign-app`, review APROVA COM NITS):** `docs/frontend.md` §4 reescrito (shell Sidebar+breadcrumbs+chip Local; Topbar+TabsBar aposentados/deletados), §4.4 registra migração visual de `/datasets`, galeria, editor e `/login` (contratos §10 intocados). Detalhes de estilo em `docs/DESIGN.md`; dívidas novas no `docs/dividas.md`.
