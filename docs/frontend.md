# Hephaestus LLM Studio — Documentação do Front-end

> Fonte: `ai-vision-training-studio.html` (protótipo single-file ~2910 linhas, marca "OmniVision Studio v1.3") + `IDEIA.md` + `arquitetura_studio_modular.png`.
> Status: **protótipo validado visualmente, não reutilizar como código final**. Alvo real: **Next.js + TypeScript**.
> Idioma da UI no protótipo: pt-BR.

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

| Aspecto | Protótipo (`ai-vision-training-studio.html`) | Alvo Next.js/TS |
|---|---|---|
| Runtime | React 18 UMD + Babel standalone + Tailwind CDN, tudo num `App()` com ~40 `useState` | App Router, componentes server/client separados, Tailwind real + CSS modules |
| Estado | Local, mockado (`INITIAL_DATASETS`, `setInterval` de 3s simulando epoch) | Server state via React Query / SWR + client state via Zustand; jobs reais via polling/WS |
| Gráficos | SVG estático com paths hardcoded (`lossGrad`, `mapGrad`, `clipGrad`) | Recharts/ECharts ou SVG próprio alimentado por `/api/jobs/:id/metrics` |
| Canvas BBox | `div`s absolutas simulando caixas | Canvas real (Fabric/Konva ou `<canvas>` próprio) com coordenadas normalizadas 0-1 |
| Upload | Botões que só disparam `showToast` | `multipart/form-data` → backend Rust, com progresso resumível |
| Logs | Array de strings com `slice(-50)` | Stream WebSocket `Rust Core → Orquestrador → Motor Python` |
| i18n/a11y | pt-BR hardcoded, bom ponto de partida a11y | Manter padrão + extrair strings |

## 3. Design system extraído do protótipo (manter)

Manter no Next.js — é a parte boa do protótipo:

- **Paleta dark-only:** fundo `#090c12`, superfícies `zinc-950/900`, acento `emerald-500 (#10b981)`, secundários `cyan/amber/rose` para classes e métricas. Variáveis CSS `:root` com `oklch()` já definidas no `<style>`.
- **Glassmorphism em 3 níveis:** `.glass-menu` (dropdowns, toast, context menu), `.glass-card` (cards, gráficos), `.glass-modal` (modal criar dataset). Todos com `backdrop-filter: blur + saturate`, borda superior mais clara (`inset 0 1px 0`), sombra profunda.
- **Tipografia:** `Inter` para UI, `JetBrains Mono / IBM Plex Mono` para números, configs, logs, YOLO TXT. Classes utilitárias `.tracking-caps` (labels uppercase) e `.tracking-display`.
- **Ícones:** set próprio `Icons` (~25 SVGs monolínea, stroke 1.7, sem emojis funcionais): Cpu, Layers, Target, Database, Wand, Play/Pause/Stop, Crosshair, BoxSelect, Terminal, Sliders, Server, Zap, etc. Migrar 1:1 para `components/icons.tsx`.
- **CTA único por painel:** 1 botão sólido `bg-emerald-500 text-zinc-950` por workspace (`start-training-btn`, `run-autotracker-btn`, `run-autolabel-btn`, `start-clip-training-btn`). Pausar = amber outline, Abortar = rose outline.
- **Feedback:** toast bottom-right (`studio-toast`, `role=status aria-live=polite`, auto-dismiss 3.6s, tipos success/error/info), menu de contexto em vidro (`glass-context-menu`), modal com `role=dialog aria-modal=true`, foco inicial via `datasetNameRef`, `Escape` fecha modal/menu/editor.
- **A11y já feita:** `focus-visible` verde 2px, `button:disabled {opacity .55}`, `prefers-reduced-motion: reduce` zerando animações (`pulse-glow`, `scan-laser`), `aria-label` em todos os selects/inputs, `role=tablist/tab aria-selected`.
- **Scrollbars finas 5px**, telemetria em `font-mono text-xs`.

Não levar para o Next.js: `tailwind.config` inline via CDN, `text/babel`, `data-od-id` (só instrumentação do protótipo).

## 4. Shell global

### 4.1 Header (`studio-topbar`, h-14, sticky)

- Logo + `Studio v1.3` + seletor de ambiente (`env-switcher-btn` → `env-dropdown-glass`): `Docker Local (RTX 4090)`, `RunPod Pod #8841 (A100)`, `VPS Dedicada (L40S)` + CTA "Conectar novo Pod / Cluster".
- Telemetria: `Rust Core: 0.2ms`, `Motor Python: PyTorch 2.4.1`, barra VRAM (`4.2/24 GB idle`, `14.8/24 GB treinando`), botão `MoreVertical` → context menu `settings`.
- No real: ambiente = `GET /api/environments` + `POST /api/environments/select`; telemetria = WS `/ws/telemetry` (Rust Core latency, VRAM, PyTorch/CUDA versão).

### 4.2 Barra de abas (`studio-tabs-bar`, h-11)

6 abas em 3 grupos (labels e badges do protótipo — manter):

- **Treino:** `difusao` (Difusão, badge `Flux·SDXL·1.5`), `openclip` (OpenCLIP, `Embedding`), `yolo` (YOLO Detecção/Tracking, `v8/v9/v11`).
- **Preparo:** `autolabel` (AutoLabel, `Difusão·CLIP`), `autotracker` (AutoTracker, `Vídeo·Imagem`).
- **Dados:** `datasets` (Datasets, badge = contagem).
- Status global à direita: "Pronto para Treinar" vs "Treinamento em Execução (Orquestrador → Motor Python)".

Roteamento sugerido (App Router): `/difusao`, `/openclip`, `/yolo`, `/autolabel`, `/autotracker`, `/datasets`, `/datasets/[id]`, `/datasets/[id]/annotate/[imageId]`. O protótipo usa `activeTab + openDatasetId + editingGalleryImage` — mapear direto para rotas.

### 4.3 Elementos transversais

- `create-dataset-modal`: backdrop `bg-black/70 backdrop-blur-sm`, campos nome (slugifica `toLowerCase().replace(/\s+/g,'-')`), tipo/tarefa (4 options), classes CSV, dropzone `.zip`/JPG/PNG/WebP + `.txt`. Submit cria `status: needs_labeling`.
- `glass-context-menu`: tipos `dataset` (Abrir galeria / Treinar neste dataset / Executar AutoLabel ou AutoTracker conforme categoria / Exportar / Excluir), `sample_image` (Corrigir BBox / Re-executar AutoTracker), `settings|import_dataset` (Selecionar backup `.zip/.json` / Reiniciar Runtime).
- Toast: manter API `showToast(message, type)`.

## 5. Datasets + Galeria + Editor (coração da IDEIA)

Exigência da IDEIA: lista/grade → clique abre galeria → só na galeria AutoLabel/AutoTracker/Exportar/Importar. O protótipo já faz exatamente isso.

### 5.1 Lista (`datasets-workspace`)

- Header "Gerenciador de Datasets" + toggle grade/lista (`viewMode`), Importar (Backup), Novo Dataset.
- Filtros: busca por nome/classe/formato + pills `Todos/Difusão/OpenCLIP/YOLO` com contadores.
- Card grade: ícone, `title`, `type`, tiles Imagens / % Rotuladas (`labeledCount/imagesCount`), chips de classes, rodapé `size · lastModified`, tag `AutoTracker` se aplicável, CTA "Treinar →" (roteia via `trainTabFor(ds)`).
- Lista: tabela Nome / Formato-Tarefa / Imagens / Progresso / Origem Storage (`source`: `/mnt/datasets/pcb`, `/workspace/drones`, `/opt/datasets`...) / Ações.
- Estado vazio com "Limpar Filtros". Clique na linha/card → `setOpenDatasetId`. Botão direito → context menu.
- Mock inicial (5 datasets, cobrir os 3 formatos): `inspecao-pcb-defeitos-v2` (yolo, ready), `drones-veiculos-urbanos-4k` (yolo, in_progress), `cyberpunk-character-lora` (difusao), `seguranca-epi-industrial` (yolo, needs_labeling), `embeddings-marcas-produtos-clip` (openclip).

### 5.2 Galeria (`dataset-gallery`)

- Breadcrumb voltar + título + meta `type · N imagens · size · source`.
- Ações: AutoLabel, AutoTracker, Exportar (`.zip + dataset.yaml + anotações`), "Treinar este Dataset".
- Faixa resumo: `N amostras · X rotuladas por AutoTracker|AutoLabel · Formato · Backup: exportar/importar mantém JSON/YAML + anotações`.
- Se `imagesCount === 0`: empty state "Galeria vazia" + "Enviar amostras" (upload Rust).
- Grade: até 8 thumbs mockadas (`img_0001.jpg`...), overlay BBox para `category==='yolo'`, selo `caption.txt` para difusão/CLIP, tile dashed "Adicionar imagens / vídeo". Clique: yolo → abre editor BBox; demais → toast "Revisão de caption no AutoLabel".

### 5.3 Editor BBox (`gallery-bbox-editor`)

- Sidebar 288px: voltar, ferramentas (`bbox B`, `select V`, `pan H`), classes do dataset (`solda_fria` emerald, `curto_circuito` amber, `componente_ausente` rose, `trilha_rompida` cyan + atalho `[1-4]`), painel "Coordenadas YOLO (Norm.)" X/Y/W/H, "Salvar Anotações".
- Canvas central com toolbar flutuante (zoom 50–250%, Reset 100%), moldura 600×450 escalada por `canvasZoom`, caixas selecionáveis com anel `ring-white/50` + alça `se-resize`.
- No real: implementar drag/resize de verdade, snap, validação `0≤x,y,w,h≤1`, atalhos B/V/H/Delete, autosave debounced → `PUT /api/datasets/:id/images/:imageId/boxes`.

## 6. Módulos de treino (1 aba por categoria)

### 6.1 YOLO (`yolo-workspace`) — preparo via AutoTracker

- Esquerda: backbone dropdown em vidro (`yolo11n.pt`, `yolo11m.pt` recomendado, `yolo11x.pt`, `yolov9-c.pt`, `yolo11-seg.pt`), dataset vinculado (só `category==='yolo`), epochs/batch (`8/16/32/64` com hints de GPU)/imgSize (`416/640/1024`)/otimizador (`AdamW/SGD/Muon`), slider `lr0` 0.0001–0.01, checks Mosaic/Mixup+Flip/Anchor-Opt, CTA Iniciar/Pausar/Abortar.
- Direita: banner status com `training-pulse` + métricas `box_loss / mAP@50 / mAP@50-95` + barra %, 2 gráficos SVG (Loss Treino×Val, mAP), grid "Inferência de Validação" (3 amostras clicáveis com bbox + `Threshold > 0.65`), terminal (`Console de Telemetria`, `Stream WebSocket Ativo`, 36 linhas, `slice(-50)`).
- Mock loop: `setInterval 3000ms` incrementa epoch, deriva loss/mAP, appenda log `[Epoch N/M] box_loss... dfl_loss... mAP50...`. Substituir por job real + WS.

### 6.2 Difusão LoRA (`difusao-workspace`) — preparo via AutoLabel

- Esquerda: base (`Flux.1-dev 12B`, `SDXL 1.0`, `SD1.5`, `Wan2.1-t2v`), dataset (só `difusao`, link "Preparar com AutoLabel →"), trigger word (`ohwx_style`), Rank (`8/16/32/64`), Alpha (`16/32/64`), otimizador (`Prodigy/AdamW8bit/Lion`), CTA Iniciar/Pausar.
- Direita: `Denoise Loss 0.0842`, `Step 450/1500`, `cyberpunk_v1.safetensors`, `Target VRAM 18.2 GB`, sandbox prompt (`{trigger}, portrait...`, CFG 3.5 Steps 24, "Gerar Preview").

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
  imagesCount: number; labeledCount: number; classes: string[];
  format: string; status: DatasetStatus; lastModified: string;
  size: string; autoTracked: boolean; source: string;
}
// ALINHADO ao contrato real (Fatia 3a — packages/contracts/openapi.yaml, ADR-0002;
// wire camelCase global): slug entra no shape (gerado pelo servidor, UNIQUE);
// size → o servidor devolve sizeBytes: number e a UI formata em lib/format.ts;
// lastModified = RFC 3339 (datasets.updated_at), não string relativa;
// type no wire é um dos 4 códigos de máquina (yolo_bbox|yolo_seg|difusao_lora|clip_image_text),
// o rótulo pt-BR é da UI; source pode ser null (sempre null na 3a);
// autoTracked é constante false até a 3d (fonte real: boxes.origin='autotracker').
interface BBox { id: number; classId: number; label: string; x: number; y: number; w: number; h: number; color: string; }
// trainTabFor(ds): difusao→/difusao, openclip→/openclip, yolo→/yolo
// selectDatasetValue(cats): filtra por categoria, fallback 1º compatível
```

## 9. Fluxos principais

1. Criar → galeria vazia → Enviar amostras → AutoLabel/AutoTracker → % rotuladas sobe → Treinar.
2. Treino YOLO: vincular dataset YOLO → hiperparams → Iniciar → monitor (epoch/loss/mAP/logs/val) → Pausar/Abortar → pesos exportados.
3. Preparo seguro: ajustar prompt/modelo/threshold no sandbox → Testar Inferência/Caption → Executar no Dataset (lote).
4. Correção manual: validação ou galeria → editor BBox → Salvar → re-treino.
5. Backup: Exportar (`.zip + dataset.yaml + anotações + captions.jsonl`) / Importar (mesmo pacote).

## 10. Contratos que o front vai exigir do Rust (alinhado com backend.md §9)

- Auth (IMPLEMENTADO Fatia 2 — `app/login/page.tsx`, `proxy.ts`, `next.config.ts`; contrato `packages/contracts/openapi.yaml`, `docs/adr/0001-auth-single-user.md`): `POST /api/auth/login`, `GET /api/auth/me`, `POST /api/auth/logout` + `GET /health` (`auth: ready|setup_required`).
  - Rota `/login`: form de senha; erros ramificados por `code` em pt-BR (`invalid_credentials` → "Senha incorreta.", `setup_required` → "Servidor em modo setup — defina STUDIO_PASSWORD.", `invalid_request` → "Envie a senha.", default → "Falha inesperada."); sucesso → `/` (`router.replace` + `refresh`); já logado (`GET /me` ok) → volta a `/`.
  - Gate de sessão via `proxy.ts`: `/login` passa direto (decide por si via `/me`); sem cookie `heph_session` → redirect `/login`; com cookie → passa, validade decidida pelo servidor via `/me` (`/` redireciona a `/login` se `/me` não-ok; logout → `POST /logout` + volta a `/login`). `/api/*` fora do matcher — envelope 401 do backend repassado intacto.
  - Resolução T5: front chama `/api/*` relativo (`credentials: "same-origin"`, sem CORS); rewrite Next → `API_INTERNAL_URL` (dev `http://localhost:8080`, compose `http://principal:8080`). `NEXT_PUBLIC_API_URL` ficou como resíduo de build (só `ARG` no Dockerfile; runtime usa o proxy `/api`).
- Datasets: `GET/POST /api/datasets`, `GET/DELETE /api/datasets/:id` — IMPLEMENTADO (Fatia 3a — contrato `packages/contracts/openapi.yaml`, ADR-0002). Upload/imagens/boxes/caption/export/import/package seguem pendentes (3b+): `POST /:id/upload` (200 MB), `GET /:id/images?limit&offset`, `PUT .../images/:img/{boxes,caption}`, `POST /:id/export`, `POST /datasets/import`, `POST /:id/package`.
- Ambientes (alias UI de orquestradores): `GET /api/environments` (= `GET /api/orchestrators`), `POST /environments/select|connect` (= adopt/enable).
- Jobs: `POST /api/jobs/{yolo|difusao|clip|autolabel|autotracker|playground}`, `GET /:id`, `POST /:id/{pause,abort,resume}`, `GET /:id/{metrics,samples,artifacts}`, `WS /ws/jobs/:id/logs?since_seq=` + `WS /ws/telemetry`.
- Runners/playground: `POST /runners/{engine}/up`, `POST /runners/:id/{kill,infer}`, `GET /runners` — infer via `POST /:id/infer`, 409 se preemptado.
- Models: `GET /api/models` (dropdowns) + `POST /models/{upload,download}`.
- Preview/sandbox: `POST /api/preview/{autolabel|autotracker|generate|search}` (efêmero, sem fila).
- Settings: chaves `hfToken, civitaiKey, openaiKey, anthropicKey, vllmEndpoint` no wire (camelCase global, ADR-0002 D1; colunas `settings` seguem snake_case) — rota ainda **não implementada** (mascaradas no GET quando chegar).

## 11. Estrutura de pastas sugerida (Next.js)

```
app/(studio)/difusao|openclip|yolo|autolabel|autotracker|datasets/page.tsx
app/datasets/[id]/page.tsx          # galeria
app/datasets/[id]/annotate/[imageId]/page.tsx  # editor BBox
components/studio/{Topbar,TabsBar,DatasetCard,DatasetTable,GalleryGrid,BBoxCanvas,MetricCard,LossChart,MapChart,RecallChart,LogTerminal,SandboxPreview,GlassMenu,Toast}.tsx
components/icons.tsx  lib/{api,ws,format}.ts  store/{studio, jobs}.ts  types/studio.ts
```

Cada workspace segue o grid do protótipo: `painel config 320–384px + área fluida p-6`, `md:flex-row` com fallback empilhado no mobile, painel com `overflow-y-auto` próprio.

## 12. Backlog front-end (pós-protótipo)

- [ ] Rotas reais + `trainTabFor` como helper de navegação.
- [ ] React Query para datasets/jobs + WS para logs/telemetria + barra VRAM.
- [ ] Canvas BBox real com drag/resize/zoom/atalhos e persistência normalizada.
- [ ] Charts reais ligados a `/metrics`; manter estilo SVG + gradiente do protótipo.
- [ ] Upload com progresso/cancel + import/export `.zip` validado.
- [ ] Formulários controlados com validação (epochs≥1, lr ranges, trigger word obrigatória).
- [ ] Testes: render das 6 abas, fluxo lista→galeria→editor, sandbox→lote, mocks de WS.

## 13. Adendos pós-revisão (decisões fechadas)

- **Auth — single-user local:** tela `/login` nova (não existe no protótipo). JWT HttpOnly, gate em todas as rotas do studio, logout no menu settings. Sem multi-user por enquanto.
- **Playground (nova aba, mesmo design):** runner sob demanda para os 3 motores — Difusão (gerar imagem), YOLO (inferência imagem/vídeo), CLIP (busca semântica). Orquestrador sobe o runner, mantém ativo até faltar VRAM ou usuário clicar "Matar runner". Card de status com VRAM usada + botão kill + aviso de preempção.
- **Samples por ciclo:** em cada treino (YOLO/Difusão/CLIP) exibir N previews geradas por época/steps com métrica + imagem — é o "health visual". Exige `GET /api/jobs/:id/samples?cycle=N` e grade na direita dos workspaces.
- **Downloads de modelos:** settings com campos HF token + Civitai key (env como fallback) + input de URL. Front só coleta e exibe progresso; download real é do orquestrador.
- **Limites:** upload avulso imagem/vídeo 200 MB (validação no front + 413 do Rust). Envio de dataset p/ orquestrador sem limite, com md5 + fragmentação quando remoto — front mostra barra de empacotamento → envio → verificação.
- **Pré-condições 3b/3d (ADR-0002 T3/T4/T7):** DELETE ganha cleanup de `<DATASETS_DIR>/<slug>` (delete-after-commit); `jobs.dataset_id ON DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`); derivar `autoTracked` de `boxes.origin='autotracker'` — detalhe no ADR.
