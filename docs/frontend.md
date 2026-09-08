# Hephaestus LLM Studio — Documentação do Front-end

> Fonte: o próprio app (`apps/web`) + `docs/design-system.md` v2 (contrato de estilo Arcane). O protótipo v1 `ai-vision-training-studio.html` está APOSENTADO como referência de layout (não removido do repo; apenas sem valor normativo).
> Status: **shell v2 implementado (fatia redesign UI v2): Sidebar macro + breadcrumbs; Topbar+TabsBar APOSENTADOS (componentes deletados)**. Alvo real: **Next.js + TypeScript**.
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

| Aspecto | Protótipo v1 (`ai-vision-training-studio.html`, APOSENTADO — comparativo histórico) | Alvo Next.js/TS |
|---|---|---|
| Runtime | React 18 UMD + Babel standalone + Tailwind CDN, tudo num `App()` com ~40 `useState` | App Router, componentes server/client separados, Tailwind real + CSS modules |
| Estado | Local, mockado (`INITIAL_DATASETS`, `setInterval` de 3s simulando epoch) | Server state via React Query / SWR + client state via Zustand; jobs reais via polling/WS |
| Gráficos | SVG estático com paths hardcoded (`lossGrad`, `mapGrad`, `clipGrad`) | Recharts/ECharts ou SVG próprio alimentado por `/api/jobs/:id/metrics` |
| Canvas BBox | `div`s absolutas simulando caixas | Canvas real (Fabric/Konva ou `<canvas>` próprio) com coordenadas normalizadas 0-1 |
| Upload | Botões que só disparam `showToast` | `multipart/form-data` → backend Rust, com resultado POR ITEM (`stored/duplicate/rejected/failed` + `reason`; sem resume na 3b — ADR-0003 D2) |
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

## 4. Shell global (v2 — fatia redesign UI v2)

> Topbar (`components/studio/Topbar.tsx`) + TabsBar (`components/studio/TabsBar.tsx`) APOSENTADOS — componentes deletados nesta fatia. O shell v2 é `Sidebar.tsx` + header com breadcrumbs + chip "Local".

### 4.1 Sidebar macro (`components/studio/Sidebar.tsx`)

- Módulos de sistema: **Dados & Anotação** ativo (`border-brand-500/30 bg-zinc-900/90` com barra lateral `bg-brand-500`, hardcoded sem `usePathname`); **Forja & Treinamento** HABILITADO (link `href="/jobs"`, `title="Forja & Treinamento"`, badge `jobsActive` quando telemetria reporta jobs ativos — `Sidebar.tsx:174-178`); **Execução & Playground** desabilitado honesto (`aria-disabled="true"`, `title="Disponível em fatia futura"`); **Configurações** desabilitado (`title="Disponível em fatia futura"`).
- Card **TELEMETRIA DO NÓ** REAL (polling `getTelemetry()` a cada 3s — `Sidebar.tsx:30-48`): VRAM com barra `vramPct` (calculada quando `measured && vramUsed && vramTotal > 0` — `Sidebar.tsx:67-69`), texto `"sem GPU (mock)"` quando `!measured` (`Sidebar.tsx:239`); CPU com barra `cpuPct` e porcentagem; RAM em GB (`ram / 1073741824`); GPUs listadas quando `gpus.length > 0` (`Sidebar.tsx:281-288`). Job counter no header do card (`telemetry.jobsActive > 0` — `Sidebar.tsx:227-231`).
- **Sair da sessão** = `POST /api/auth/logout` (`credentials: "same-origin"`, erro ignorado) + `router.replace("/login")` + `router.refresh()` — mecanismo migrado da Topbar (`Sidebar.tsx:51-63`).
- Drawer mobile: largura `w-[min(85vw,320px)]`, backdrop `bg-black/70 backdrop-blur-sm` (`lg:hidden`), fecha em `Escape` (listener em `(studio)/layout.tsx:31-38`) e em navegação (`setSidebarOpen(false)` no `pathname`).

### 4.2 Header do shell (`app/(studio)/layout.tsx`)

- Breadcrumbs em 1 linha via `usePathname` (segmentos com `truncate`, `max-w-[140px]` no meio; raiz = "Studio").
- Chip estático **"Local"** (`title="Nó local — ambiente único nesta fatia"`) — sem dropdown funcional, não há endpoint de ambientes nesta fatia.

### 4.3 Elementos transversais

- `create-dataset-modal`: backdrop `bg-black/70 backdrop-blur-sm`, campos nome (slugifica `toLowerCase().replace(/\s+/g,'-')`), tipo/tarefa (4 options), classes CSV, dropzone `.zip`/JPG/PNG/WebP + `.txt`. Submit cria `status: needs_labeling`.
- `glass-context-menu`: tipos `dataset` (Abrir galeria / Treinar neste dataset / Executar AutoLabel ou AutoTracker conforme categoria — AutoTracker habilitado para `category==='yolo'` com ≥1 classe e ≥1 imagem; AutoLabel continua desabilitado / Exportar / Excluir), `sample_image` (Corrigir BBox / Re-executar AutoTracker — re-executar por imagem permanece futuro, não habilitado na v1), `settings|import_dataset` (Selecionar backup `.zip/.json` / Reiniciar Runtime).
- Toast: manter API `showToast(message, type)` (restyle v2; toast com ação vive 6s — Desfazer da 3g).

### 4.4 Páginas no estilo v2 (APRESENTAÇÃO apenas — contratos §10 e lógica intocados)

`/datasets`, galeria (`datasets/[id]`), editor BBox (`annotate/[imageId]`) e `/login` migrados para o v2 (`docs/design-system.md`): densidade de botões (CTA único `h-11`, secundárias `h-9`, menu overflow `"⋯"` em `<md`), pílulas de categoria com `overflow-x-auto` + fade edge + auto-scroll da pílula ativa, anti-scroll-trap (workspace rola como documento único em `<md`; scroll interno de coluna só em `≥md` com `md:overflow-y-auto`).

## 5. Datasets + Galeria + Editor (coração da IDEIA)

Exigência da IDEIA: lista/grade → clique abre galeria → só na galeria AutoLabel/AutoTracker/Exportar/Importar. O protótipo já faz exatamente isso.

### 5.1 Lista (`datasets-workspace`)

- Header "Gerenciador de Datasets" + toggle grade/lista (`viewMode`), Novo Dataset. (O botão "Importar (Backup)" do header foi REMOVIDO na Fatia 3e — P2 da ADR-0006: import só na galeria.)
- Filtros: busca por nome/classe/formato + pills `Todos/Difusão/OpenCLIP/YOLO` com contadores.
- Card grade: ícone, `title`, `type`, tiles Imagens / % Rotuladas (`labeledCount/imagesCount`), chips de classes, rodapé `size · lastModified`, tag `AutoTracker` se aplicável, CTA "Treinar →" (roteia via `trainTabFor(ds)`).
- Lista: tabela Nome / Formato-Tarefa / Imagens / Progresso / Origem Storage (`source` derivado: `null` em dataset vazio, `s3://{bucket}/datasets/{id}/` com imagens — ADR-0003 D5) / Ações.
- Estado vazio com "Limpar Filtros". Clique na linha/card → `setOpenDatasetId`. Botão direito → context menu.
- Mock inicial (5 datasets, cobrir os 3 formatos): `inspecao-pcb-defeitos-v2` (yolo, ready), `drones-veiculos-urbanos-4k` (yolo, in_progress), `cyberpunk-character-lora` (difusao), `seguranca-epi-industrial` (yolo, needs_labeling), `embeddings-marcas-produtos-clip` (openclip).

### 5.2 Galeria (`dataset-gallery`)

- Breadcrumb voltar + título + meta `type · N imagens · size · source`.
- Ações — Exportar/Importar IMPLEMENTADOS (Fatia 3e, ADR-0006): Exportar baixa o `.zip` do backup via blob + `Content-Disposition` (`lib/backup.ts::exportDataset`, toasts 404/503); Importar abre o `ImportDatasetModal` (file picker `.zip` + campo nome opcional ≤96, toasts por `code` em 400/503/413, resultado com contagens + "Abrir dataset importado"); **fluxo de substituição: 409 `slug_conflict` → diálogo de irreversibilidade ("substituir apaga o dataset atual; a ação não tem reversão", Cancelar/Substituir) → re-envio com `replace=true` (o 409 NÃO é toast)**. Pacote = `manifest.json` (fonte da verdade) + `dataset.yaml`/`labels/*.txt`/`captions.jsonl` derivados + `images/*`. Demais ações: AutoLabel, AutoTracker, "Treinar este Dataset".
- Faixa resumo: `N amostras · X rotuladas por AutoTracker|AutoLabel · Formato · Backup: exportar/importar mantém JSON/YAML + anotações`.
- **AutoTracker — IMPLEMENTADO (Fatia 5, ADR-0008):** ação na galeria abre `AutoTrackerModal` (só para `category==='yolo'` com `classes.length>0 && imagesCount>0`; senão disabled com title honesto). Modal: modelo fixo `mock`, slider `conf` 0.3–0.95 (default 0.65), **SEM checkbox de overwrite** (decisão de apply é única no card de `/jobs`, default `overwrite=false`). Submit: `startAutotrackerJob({datasetId, model:"mock", conf})` → `POST /api/jobs/autotracker` → 202 → toast de sucesso → navega para `/jobs`.
- Se `imagesCount === 0`: empty state "Galeria vazia" + "Enviar amostras" (upload Rust).
- Grade: até 8 thumbs mockadas (`img_0001.jpg`...), overlay BBox para `category==='yolo'`, selo `caption.txt` para difusão/CLIP, tile dashed "Adicionar imagens / vídeo". Clique: yolo → abre editor BBox; demais → toast "Revisão de caption no AutoLabel".
- Gestão de classes e amostras — IMPLEMENTADO (Fatia 3g): botão "Classes" abre o `ClassesModal` (renomear/adicionar/remover; 409 mantém o modal aberto com estado); hover por thumb mostra trash (soft delete → toast com **Desfazer**); pills `Ativas | Lixeira (n)` (`n` = `Dataset.trashCount`); na lixeira cada item tem `Restaurar` (conflito de filename → servidor renomeia `_restaurado` e devolve `{filename}`) e o botão `Esvaziar` (= única exclusão permanente, com `ConfirmDialog`, chama `DELETE /:id/trash`).
- Painel de busca semântica — IMPLEMENTADO (Fatia 3f, spec 0.5.0, ADR-0004): barra de texto na galeria (só na visão `ativas`; `maxLength={500}`, submit → `searchDataset` em `lib/search.ts`) + botão "Buscar"/"Buscando…"; hover por thumb mostra lupa ("Buscar similares" → `searchByImage` com o `imageId` do item); grade de resultados com score mono (`score.toFixed(2)`, title "Similaridade (cosseno, -1..1)", clique → editor/caption) + "Limpar busca" e empty state ("Nenhum resultado para '…'"/"Nenhuma imagem similar"); badge de status com os 4 estados (`Sem índice` + botão "Indexar agora" em `not_indexed`; `Indexando indexedCount/imagesCount` em `indexing`; `ready`; `stale` tipado mas nunca emitido) com polling a cada 2s enquanto `indexing` (+ `Status indisponível` em falha); "Indexar agora" dispara `POST …/search/index` (`triggerSearchIndex`) e vira `Indexando…`.

### 5.3 Editor BBox (`gallery-bbox-editor`)

- Sidebar 288px: voltar, ferramentas (`bbox B`, `select V`, `pan H`), classes do dataset (`solda_fria` emerald, `curto_circuito` amber, `componente_ausente` rose, `trilha_rompida` cyan + atalho `[1-4]`), painel "Coordenadas YOLO (Norm.)" X/Y/W/H, "Salvar Anotações".
- Canvas central com toolbar flutuante (zoom 50–250%, Reset 100%), moldura 600×450 escalada por `canvasZoom`, caixas selecionáveis com anel `ring-white/50` + alça `se-resize`.
- No real: implementar drag/resize de verdade, snap, validação `0≤x,y,w,h≤1`, atalhos B/V/H/Delete, autosave debounced → `PUT /api/datasets/:id/images/:imageId/boxes`.
- Gerenciar classes no editor — IMPLEMENTADO (Fatia 3g): botão "Gerenciar classes" abre o mesmo `ClassesModal` da galeria (copy nova: renomear preserva caixas; remover classe com caixas é bloqueado com 409); o editor NÃO tem exclusão de imagem (um lugar só destrói: a galeria).

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

- Auth (IMPLEMENTADO Fatia 2 — `app/login/page.tsx`, `proxy.ts`, `next.config.ts`; contrato `packages/contracts/openapi.yaml`, `docs/adr/0001-auth-single-user.md`): `POST /api/auth/login`, `GET /api/auth/me`, `POST /api/auth/logout` + `GET /health` (`auth: ready|setup_required`).
  - Rota `/login`: form de senha; erros ramificados por `code` em pt-BR (`invalid_credentials` → "Senha incorreta.", `setup_required` → "Servidor em modo setup — defina STUDIO_PASSWORD.", `invalid_request` → "Envie a senha.", default → "Falha inesperada."); sucesso → `/` (`router.replace` + `refresh`); já logado (`GET /me` ok) → volta a `/`.
  - Gate de sessão via `proxy.ts`: `/login` passa direto (decide por si via `/me`); sem cookie `heph_session` → redirect `/login`; com cookie → passa, validade decidida pelo servidor via `/me` (`/` redireciona a `/login` se `/me` não-ok; logout → `POST /logout` + volta a `/login`). `/api/*` fora do matcher — envelope 401 do backend repassado intacto.
  - Resolução T5: front chama `/api/*` relativo (`credentials: "same-origin"`, sem CORS); rewrite Next → `API_INTERNAL_URL` (dev `http://localhost:8080`, compose `http://principal:8080`). `NEXT_PUBLIC_API_URL` ficou como resíduo de build (só `ARG` no Dockerfile; runtime usa o proxy `/api`).
- Datasets: `GET/POST /api/datasets`, `GET/DELETE /api/datasets/:id` — IMPLEMENTADO (Fatia 3a — contrato `packages/contracts/openapi.yaml`, ADR-0002). Upload/imagens/boxes/caption — IMPLEMENTADO (Fatia 3b — spec 0.3.0, contrato que a 3c/3d implementa; as rotas de UI que os consomem ainda NÃO existem — `/datasets` é a 3c): `POST /:id/upload` (multipart `files`; corpo total 200 MiB + 8 MiB envelope → 413; teto por arquivo 200 MiB → item `rejected/too_large`; resposta `{items:[{imageId,filename,status,reason,bytes,width,height}]}` camelCase), `GET /:id/images?limit(=50, máx 200)&offset(=0)&split(train|val)&labeled(bool)` → `{items,total,limit,offset}`, `GET /:id/images/:imageId` (Image flat + `boxes[]` + `caption|null`), `GET /:id/images/:imageId/data` (proxy incondicional, `Cache-Control: private, max-age=31536000, immutable`), `PUT .../images/:imageId/boxes` (`{boxes:[{classId,x,y,w,h,conf?,origin?,trackId?}]}` cap 1000, domínio 0..1), `PUT .../images/:imageId/caption` (upsert `{text:1..8000,origin?,model?≤255}`).   Export/import — IMPLEMENTADO (Fatia 3e, spec 0.6.0, ADR-0006): `POST /:id/export` (download `.zip` via blob + `Content-Disposition`), `POST /datasets/import` (FormData `file` + `title` opcional + `replace: "true"` só quando true; fluxo 409 → diálogo de irreversibilidade → `replace=true`); `lib/backup.ts` (`exportDataset`/`importDataset` + copies por `code`) + `components/studio/ImportDatasetModal.tsx`; limite do zip 200 MiB + 8 MiB envelope (413). `POST /:id/package` — IMPLEMENTADO (Fatia 4, spec 0.7.0, ADR-0007 D1): congela `dataset_versions`, gera zip, PUT `packages/<version_id>/`, 200 `PackageResponse{versionId,key,bytes,md5Zip,files}`.
- Ambientes (alias UI de orquestradores): `GET /api/environments` (= `GET /api/orchestrators`), `POST /environments/select|connect` (= adopt/enable).
- Jobs — IMPLEMENTADO (Fatia 4; spec 0.7.0, ADR-0007):
  - Rota nova `/jobs` (Forja & Treinamento, Sidebar ativa — `Sidebar.tsx:158-187`, link `href="/jobs"` com badge `jobsActive` em tempo real).
  - `POST /api/jobs/yolo` (`lib/jobs.ts:startYoloJob`): body `{datasetId,model,epochs,batch,imgsz,lr0,optimizer,augment}`, 202 `{jobId,status:"queued",queuePosition?}`. Modal `TrainYoloModal` (condição: dataset yolo com ≥1 classe e ≥1 imagem — verificação no botão Treinar da galeria e do `DatasetMenu`).
  - `GET /api/jobs` (`lib/jobs.ts:listJobs`): response camelCase `{items:[Job],total}` (JobList). Polling 3s ativo quando há jobs `queued`/`running`/`cancelling`; pausa quando todos terminam (`jobs/page.tsx:100-126`).
  - `GET /api/jobs/:id/metrics` (`lib/jobs.ts:getJobMetrics`): `{items:[{epoch,boxLoss,clsLoss,dflLoss,map50,map5095}]}` (camelCase wire; `mAP50-95` → `map5095`). Série por epoch, vazia se job sem métricas.
  - `GET /api/jobs/:id/artifacts` (`lib/jobs.ts:getJobArtifacts`): `{items:[{id,kind,path,md5,bytes}]}`.
  - `GET /api/jobs/:id/artifacts/:artifactId/data` (`lib/jobs.ts:downloadArtifact`): download via blob + `document.createElement("a")` + `URL.createObjectURL`.
  - `POST /api/jobs/:id/abort` (`lib/jobs.ts:abortJob`): 200 `{"status":"cancelling"|"cancelled"}` ou 409 `job_not_abortable`. UI: `ConfirmDialog` antes de abortar.
  - `GET /api/jobs/queue` (`lib/jobs.ts:listQueue` — não exposta na UI v1, mas disponível): `{items:[{jobId,position,queueReason}]}`.
  - AutoTracker — IMPLEMENTADO (Fatia 5, ADR-0008, spec 0.8.0):
    - `POST /api/jobs/autotracker` (`lib/autotracker.ts:startAutotrackerJob`): body `{datasetId, model?, conf?}`, 202 `{jobId,status:"queued",queuePosition?}`. Modal `AutoTrackerModal` (model fixo `mock`, slider `conf` 0.3–0.95 default 0.65, **sem checkbox de overwrite** — decisão única no card de apply).
    - `POST /api/jobs/:id/autotracker/apply` (`lib/autotracker.ts:applyAutotrackerBoxes`): body `{overwrite?, imageId?}`, 200 `{applied, skipped, images}`; 409 `job_not_done` (job não está `done`); 400 `invalid_request` (imageId não-UUID); 404 `not_found`; 503 `queue_unavailable`/`storage_unavailable`.
    - Rota `/jobs` reestruturada (Fatia 5): setup central `ForjaYoloSetup` + coluna Atividade à esquerda; botão **"Aplicar boxes ao dataset"** habilitado quando `status==='done' && engine==='autotracker'` + artefato `boxes.json` existe, com checkbox `overwrite` e desabilitado sem artefato boxes.json; `usePathname` para highlight do módulo ativo na Sidebar.
  - **Nota wire snake_case:** `GET /api/jobs/:id` devolve o payload interno do manager com campos snake_case (`queue_reason`, `queue_position`), mas a UI v1 só consome `status`, `progress`, `epoch`, `step`, `metrics`, `createdAt`, `finishedAt` — todos camelCase ou primitivos.
  - Telemetria real (polling Sidebar `getTelemetry()` a cada 3s — `Sidebar.tsx:30-48`): `Telemetry{measured,cpu,ram,vramUsed,vramTotal,gpus,jobsActive}`. CPU/RAM do `/proc` do container orquestrador (medido por heartbeat ~2s); VRAM `"sem GPU (mock)"` quando `!measured` (caminho real: `measured:false` sempre no mock local — `Sidebar.tsx:239`); GPU listada só quando `gpus.length > 0` (`Sidebar.tsx:281`). **Nota:** o texto "sem GPU (mock)" só aparece com `measured:false`, que NÃO ocorre na v1 real (o mock sempre reporta `measured:false` — caminho morto documentado na ADR-0007 D9).
- Runners/playground: `POST /runners/{engine}/up`, `POST /runners/:id/{kill,infer}`, `GET /runners` — infer via `POST /:id/infer`, 409 se preemptado.
- Models: `GET /api/models` (dropdowns) + `POST /models/{upload,download}`.
- Preview/sandbox: `POST /api/preview/{autolabel|autotracker|generate|search}` (efêmero, sem fila).
- Settings: chaves `hfToken, civitaiKey, openaiKey, anthropicKey, vllmEndpoint` no wire (camelCase global, ADR-0002 D1; colunas `settings` seguem snake_case) — rota ainda **não implementada** (mascaradas no GET quando chegar).
- Classes e lixeira — IMPLEMENTADO (Fatia 3g, spec 0.4.0, ADR-0005): `putClasses(datasetId, classes)` (`lib/classes.ts` → `PUT /:id/classes`, reconciliação por id, 409 `classes_in_use` mantém o modal aberto); `softDeleteImage` (`DELETE /:id/images/:imageId` → 204, sem sweep) / `restoreImage` (`POST .../restore` → 204 sem conflito | 200 `{filename}` com rename `_restaurado`) / `purgeTrash` (`DELETE /:id/trash` → 204) (`lib/images.ts`; listagem da lixeira via `listImages(id, {deleted:true})`); `Toast.action` (`{label, onClick}`, toast com ação vive 6s — usado pelo Desfazer).
- Busca semântica — IMPLEMENTADO (Fatia 3f, spec 0.5.0, ADR-0004): `searchDataset(datasetId, q, {k?, classId?, split?})` (`lib/search.ts` → `GET /:id/search?q&k&classId&split`, 200 `SearchResponse` | 400 inválida | 409 `index_not_ready` | 503 `embedding_unavailable`), `searchByImage(datasetId, imageId, {k?, threshold?})` (`lib/search.ts` → `POST /:id/search/by-image {imageId,k?,threshold?}`, 200 `SearchResponse` | 400 corpo inválido | 404 imagem fora do dataset | 409 `index_not_ready`), `getSearchStatus(datasetId)` (`lib/search.ts` → `GET /:id/search/status`, 200 `SearchStatus{status,imagesCount,indexedCount,model,dim}`), `triggerSearchIndex(datasetId)` (`lib/search.ts` → `POST /:id/search/index`, 202 `{status:indexing|not_indexed}`) (`types/studio.ts`: `SearchIndexStatus = "not_indexed"|"indexing"|"ready"|"stale"`, `SearchStatus`, `SearchItem{image,score}`, `SearchResponse{items}`); campo da imagem nos resultados é `url` (o mesmo `Image.url` do list — nunca `thumbUrl`).

## 11. Estrutura de pastas sugerida (Next.js)

```
app/(studio)/difusao|openclip|yolo|autolabel|autotracker|datasets/page.tsx
app/datasets/[id]/page.tsx          # galeria
app/datasets/[id]/annotate/[imageId]/page.tsx  # editor BBox
components/studio/{Sidebar,DatasetCard,DatasetTable,GalleryGrid,BBoxCanvas,MetricCard,LossChart,MapChart,RecallChart,LogTerminal,SandboxPreview,GlassMenu,Toast}.tsx
# (Topbar e TabsBar APOSENTADOS e deletados na fatia redesign UI v2 — ver §4.)
components/icons.tsx  lib/{api,ws,format}.ts  store/{studio, jobs}.ts  types/studio.ts
```

Cada workspace segue o grid do protótipo: `painel config 320–384px + área fluida p-6`, `md:flex-row` com fallback empilhado no mobile, painel com `overflow-y-auto` próprio.

## 12. Backlog front-end (pós-protótipo)

- [x] Rotas reais + `trainTabFor` como helper de navegação (Fatia 4 — `/jobs` com polling, `TrainYoloModal`).
- [x] React Query para datasets/jobs + polling para jobs/telemetria (Fatia 4 — `lib/jobs.ts` com polling 3s, `Sidebar.tsx` com telemetry poll; React Query fica para refactor futuro).
- [ ] Canvas BBox real com drag/resize/zoom/atalhos e persistência normalizada.
- [ ] Charts reais ligados a `/metrics`; manter estilo SVG + gradiente do protótipo.
- [x] Import/export `.zip` validado (Fatia 3e — galeria + modal + toasts por `code`).
- [ ] Upload com progresso/cancel.
- [ ] Formulários controlados com validação (epochs≥1, lr ranges, trigger word obrigatória).
- [ ] Testes: render das 6 abas, fluxo lista→galeria→editor, sandbox→lote, mocks de WS.

## 13. Adendos pós-revisão (decisões fechadas)

- **Auth — single-user local:** tela `/login` nova (não existe no protótipo). JWT HttpOnly, gate em todas as rotas do studio, logout no menu settings. Sem multi-user por enquanto.
- **Playground (nova aba, mesmo design):** runner sob demanda para os 3 motores — Difusão (gerar imagem), YOLO (inferência imagem/vídeo), CLIP (busca semântica). Orquestrador sobe o runner, mantém ativo até faltar VRAM ou usuário clicar "Matar runner". Card de status com VRAM usada + botão kill + aviso de preempção.
- **Samples por ciclo:** em cada treino (YOLO/Difusão/CLIP) exibir N previews geradas por época/steps com métrica + imagem — é o "health visual". Exige `GET /api/jobs/:id/samples?cycle=N` e grade na direita dos workspaces.
- **Downloads de modelos:** settings com campos HF token + Civitai key (env como fallback) + input de URL. Front só coleta e exibe progresso; download real é do orquestrador.
- **Limites:** upload avulso imagem/vídeo 200 MB por arquivo (validação no front + item `rejected/too_large` e 413 do Rust no corpo total); zip de import: 200 MiB + 8 MiB de envelope (413 do Rust no corpo total; modal mostra "Backup maior que o limite de 200 MiB."). Envio de dataset p/ orquestrador sem limite, com md5 + fragmentação quando remoto — front mostra barra de empacotamento → envio → verificação.
- **Pré-condições 3b/3d (ADR-0002 T3/T4/T7):** 3b ENTREGOU storage+imagens+anotação (upload/imagens/boxes/caption + `source` derivado + sweep de prefixo); export/import/package → 3e. DELETE ganhou sweep de prefixo `datasets/{id}/` pós-commit reapável best-effort (não mais cleanup de `<DATASETS_DIR>/<slug>` — disco morreu, ADR-0003 D7); `jobs.dataset_id ON DELETE SET NULL` + snapshot `dataset_versions` (nunca `RESTRICT`); derivar `autoTracked` de `boxes.origin='autotracker'` — detalhe no ADR.
- **Fatia redesign UI v2 (branch `feature/redesign-app`, review APROVA COM NITS):** `docs/frontend.md` §4 reescrito (shell Sidebar+breadcrumbs+chip Local; Topbar+TabsBar aposentados/deletados), §4.4 registra migração visual de `/datasets`, galeria, editor e `/login` (contratos §10 intocados). Detalhes de estilo no `docs/design-system.md` v2; dívidas novas no `docs/dividas.md`.
