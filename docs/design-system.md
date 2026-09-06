---
name: Hephaestus LLM Studio
description: Estúdio de visão computacional, preparo de datasets e orquestração de treino
colors:
  primary: "#10b981"
  primary-glow: "rgba(16, 185, 129, 0.22)"
  primary-hover: "#059669"
  bg-base: "#090c12"
  surface-card: "rgba(18, 23, 35, 0.62)"
  surface-menu: "rgba(13, 17, 26, 0.82)"
  surface-modal: "rgba(11, 15, 24, 0.88)"
  fg-default: "#f1f3f9"
  fg-muted: "rgba(241, 243, 249, 0.65)"
  border-default: "rgba(255, 255, 255, 0.09)"
  border-card: "rgba(255, 255, 255, 0.07)"
  status-success: "#34d399"
  status-alert: "#f59e0b"
  status-danger: "#ef4444"
  status-telemetry: "#06b6d4"
typography:
  display:
    fontFamily: "Inter, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  headline:
    fontFamily: "Inter, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Inter, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "normal"
  body:
    fontFamily: "Inter, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "normal"
  label:
    fontFamily: "JetBrains Mono, 'IBM Plex Mono', ui-monospace, monospace"
    fontSize: "0.6875rem"
    fontWeight: 500
    lineHeight: 1.4
    letterSpacing: "0.08em"
rounded:
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#090c12"
    rounded: "{rounded.md}"
    padding: "8px 16px"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
  card-glass:
    backgroundColor: "{colors.surface-card}"
    rounded: "{rounded.xl}"
    padding: "16px"
  input-text:
    backgroundColor: "rgba(0, 0, 0, 0.4)"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
---

# Design System: Hephaestus LLM Studio

> **Referências canônicas:** `ai-vision-training-studio.html` (protótipo v1 — referência primária de layout), `ai-vision-training-studio-v2.html` (protótipo v2 — referência canônica da tela /login), `apps/web/app/globals.css` (tokens vivos), `docs/frontend.md` (contratos §10).
>
> **Fonte de verdade de ESTILO para @frontend-dev e @ui-designer.**

## Overview

**Creative North Star: "The Optical Foundry"**

O Hephaestus LLM Studio projeta a precisão de um laboratório de fundição e ótica industrial de ponta. É um ambiente de engenharia especializado em visão computacional e inteligência artificial generativa, concebido para operadores, pesquisadores e engenheiros de machine learning que passam horas refinando dados, rotulando mídias e monitorando jobs de GPU. Cada elemento na tela responde diretamente a uma necessidade funcional ou operacional; não há espaço para decorações gratuitas, gradientes genéricos ou emojis funcionais ("Anti-Slop").

A atmosfera visual é dominada pelo contraste rigoroso entre o fundo escuro profundo (`#090c12`), superfícies translúcidas com textura óptica de vidro fumê e o pulsar tático do acento Esmeralda (`#10b981`). A densidade de dados é equilibrada por uma tipografia dual: `Inter` para clareza estrutural e legibilidade de controles, e `JetBrains Mono` para todo dado numérico, telemetria, timestamp, log de terminal ou rótulo de classe em caixa alta.

**Key Characteristics:**
- **Estética Dark-Only de Alta Fidelidade:** Protege contra fadiga ocular e destaca inspeções visuais em imagens e caixas delimitadoras (BBoxes).
- **Vidro Óptico em 3 Níveis:** Hierarquia dimensional transparente com desfoque gaussiano balanceado e bordas iluminadas, sem recurso a sombras pesadas e turvas.
- **Economia de Ação Estrita:** Exatamente um botão primário sólido esmeralda por painel de controle, orientando o foco imediato do usuário.
- **Microtipografia Técnica Exata:** Rótulos funcionais em tracking expandido (`letter-spacing: 0.08em`) e números mono-espaçados que não sofrem variação de largura tabular.

## Colors

Paleta dark-only profunda fundamentada em OKLCH com equivalente hexadecimal, priorizando contraste cirúrgico, distinção visual de estados e legibilidade de classes.

### Primary
- **Esmeralda Técnico** (`#10b981` / `oklch(68% 0.20 150)`): Ação primária, confirmação de prontidão, anel de foco WCAG e pulso de execução ativo.
- **Halo Esmeralda Glow** (`rgba(16, 185, 129, 0.22)` / `oklch(68% 0.20 150 / 22%)`): Aura sutil em volta de elementos em execução ativa e indicadores de foco.

### Secondary
- **Verde Sucesso / Ativo** (`#34d399`): Barras de progresso de treino, classe 1 de anotação (`solda_fria`) e confirmações de status semântico.
- **Âmbar Pausa / Alerta** (`#f59e0b`): Ação de pausar treino, avisos de threshold e classe 2 de anotação (`curto_circuito`).
- **Amarelo Runtime Python** (`#eab308` / `yellow-400`): Ponto de status do Motor PyTorch na telemetria (protótipos v1/v2 linha 864).
- **Rosa Perigo / Abortar** (`#ef4444`): Ação de abortar ou deletar dataset, curvas de Loss e classe 3 de anotação (`componente_ausente`).
- **Ciano Telemetria** (`#06b6d4`): Latência do Rust Core, selos AutoTracker e classe 4 de anotação (`trilha_rompida`).

### Neutral
- **Fundo Base** (`#090c12` / `oklch(13.5% 0.012 260)`): Pano de fundo geral da viewport.
- **Superfície Card** (`rgba(18, 23, 35, 0.62)`): Cartões de datasets, cards de métricas e painéis de treino.
- **Superfície Menu & Topbar** (`rgba(13, 17, 26, 0.82)`): Dropdowns suspensos, toast e barra superior.
- **Superfície Modal** (`rgba(11, 15, 24, 0.88)`): Diálogos centrais de criação e confirmação.
- **Texto Principal** (`#f1f3f9` / `oklch(96% 0.005 260)`): Títulos, valores de destaque e rótulos de controle ativo.
- **Texto Secundário / Muted** (`rgba(241, 243, 249, 0.65)` / `oklch(65% 0.015 260)`): Labels explicativos, telemetria secundária e metadados.
- **Borda Estrutural** (`rgba(255, 255, 255, 0.09)`): Divisores, contornos de inputs e separadores de coluna.

### Named Rules
**The One CTA Rule.** Existe apenas um botão sólido verde esmeralda (`bg-emerald-500 text-zinc-950 font-medium`) visível como ação definitiva em cada painel (ex.: "Iniciar Treinamento", "Salvar Alterações"). Controles secundários utilizam acabamento ghost, vidro sutil ou contorno fino.
**The Class Palette Integrity Rule.** As cores das classes de detecção (verde, âmbar, rosa, ciano) são reservadas e nunca reutilizadas para indicar estados de interface conflitantes na mesma área visual.

## Typography

**Display Font:** `Inter`, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif  
**Body Font:** `Inter`, -apple-system, blinkmacsystemfont, 'Segoe UI', roboto, sans-serif  
**Label/Mono Font:** `JetBrains Mono`, `IBM Plex Mono`, ui-monospace, monospace  

**Character:** Pareamento utilitário suíço-industrial que transmite seriedade operacional na interface geral e rigor matemático absoluto em telemetria, logs e anotações.

### Hierarchy
- **Display** (SemiBold 600, 1.5rem / 24px, line-height 1.2, tracking -0.02em): Títulos principais de tela, nome da aplicação no shell e cabeçalhos de workspaces.
- **Headline** (SemiBold 600, 1.125rem / 18px, line-height 1.3, tracking -0.01em): Títulos de seções de workspace e nomes de datasets.
- **Title** (SemiBold 600, 0.875rem / 14px, line-height 1.4): Títulos de cards de parâmetros e labels principais de grupos de campos.
- **Body** (Regular 400, 0.875rem / 14px, line-height 1.6): Textos de descrição, explicações de status e instruções inline (comprimento máx. 65-75ch).
- **Label / Micro-Caps** (Medium 500, 0.6875rem / 11px, line-height 1.4, tracking 0.08em uppercase, Mono): Rótulos de formulário, identificadores de classes, badges de status e telemetria.

### Named Rules
**The Monospace Truth Rule.** Todo número representando medição (VRAM, latência, tempo de epoch, dimensões em pixels, coordenadas de bounding box e loss) deve ser renderizado obrigatoriamente em fonte monoespaçada para garantir alinhamento tabular e evitar jittering visual durante atualizações ao vivo.

## Layout

O estúdio opera em um modelo espacial de precisão, composto por:
- **Shell Global Fixo:** Topbar sticky (`h-14`, 56px) com indicador de ambiente e telemetria, seguido da barra de abas modular (`h-11`, 44px) agrupando Treino, Preparo e Dados.
- **Divisão de Workspace em 2 Colunas:**
  - *Coluna de Controle/Configuração:* Largura fixa entre 320px e 384px (`w-full md:w-80 lg:w-96`) com rolagem interna isolada. Concentra seletores, hiperparâmetros e o CTA principal.
  - *Coluna de Monitoramento/Anotação:* Painel fluido responsivo preenchendo o restante da viewport (`p-4 md:p-6`), contendo gráficos de convergência, canvas de imagem/vídeo ou terminal de streaming de logs.
- **Espaçamento e Ritmo:** Grade de 4px/8px. Gaps padrão de `12px` a `16px` entre cards de parâmetros e `24px` entre seções estruturais.

## Elevation & Depth

O sistema rejeita sombras difusas cinzentas ou pretas sólidas. A profundidade é produzida por **Vidro Óptico Translúcido (Frosted Glassmorphism)** em 3 níveis com refração de borda superior:

### Shadow & Glass Vocabulary
- **Nível 1 (.glass-card):** `backdrop-filter: blur(20px) saturate(160%)`, borda `1px solid rgba(255,255,255,0.07)` com topo iluminado `border-top: 1px solid rgba(255,255,255,0.13)` e sombra rasa `0 12px 30px -6px rgba(0,0,0,0.45)`. Usado em cards de métricas, painéis de parâmetros e itens de grid de datasets.
- **Nível 2 (.glass-menu):** `backdrop-filter: blur(28px) saturate(190%)`, borda topo `rgba(255,255,255,0.24)` e sombra profunda `0 24px 60px -8px rgba(0,0,0,0.85)`. Usado em dropdowns suspensos, menus de contexto do botão direito e toasts flutuantes.
- **Nível 3 (.glass-modal):** `backdrop-filter: blur(32px) saturate(200%)`, borda topo `rgba(255,255,255,0.28)` e sombra maciça `0 30px 80px -12px rgba(0,0,0,0.90)`. Usado em caixas de diálogo centrais com backdrop escurecido.

### Named Rules
**The Refractive Edge Rule.** Toda superfície elevada de vidro possui sua borda superior (`border-top`) com opacidade de iluminação de 1.8x a 2.5x maior que as bordas laterais e inferiores, simulando reflexão ótica de luz zenital.

## Shapes

- **Bordas e Cantos:**
  - `rounded-sm` (4px - 6px): Badges em mono, microtags e rótulos de BBox no canvas.
  - `rounded-md` (8px): Botões de ação, inputs de formulário, switches e selects.
  - `rounded-xl` (12px - 16px): Cards de métricas, blocos de parâmetros e painéis laterais.
  - `rounded-2xl` (16px - 20px): Menus flutuantes e modais de diálogo.
  - `rounded-full` (9999px): Indicadores de status pulsante e chips de filtros globais.
- **Canvas BBox:** Caixas delimitadoras com borda sólida de 1.5px na cor da classe correspondente, alças quadradas de redimensionamento nos vértices (`w-2 h-2`) e tag identificadora ancorada no vértice superior esquerdo.

## Components

### Buttons
- **Primary:** `bg-emerald-500 text-zinc-950 px-4 py-2 text-xs font-medium rounded-lg hover:bg-emerald-400 transition-colors shadow-sm`.
- **Secondary / Ghost:** `bg-zinc-900/60 border border-zinc-700/80 text-zinc-200 px-3 py-1.5 text-xs rounded-lg hover:bg-zinc-800`.
- **Destructive / Abort:** `border border-rose-500/60 text-rose-400 bg-rose-950/20 px-3 py-1.5 text-xs rounded-lg hover:bg-rose-950/40`.
- **Warning / Pause:** `border border-amber-500/60 text-amber-400 bg-amber-950/20 px-3 py-1.5 text-xs rounded-lg hover:bg-amber-950/40`.

### Chips & Badges
- **Status Badge:** Cápsula pill com borda fina translúcida e ponto luminoso pulsante (`w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse`).
- **Telemetry Capsule:** `bg-black/40 border border-white/10 px-2.5 py-1 text-zinc-300 font-mono text-[11px] rounded-full`.

### Cards / Containers
- Base `.glass-card` com preenchimento interno padronizado (`p-4` a `p-6`), cabeçalho de título com tracking sutil e separador `border-b border-white/5` opcional.

### Inputs & Fields
- `bg-black/40 border border-zinc-800 text-zinc-100 rounded-lg px-3 py-1.5 text-xs font-mono focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-500`.

### Navigation
- Topbar sticky com seletor de pods em dropdown; barra de abas com divisores verticais sutis entre os grupos de Difusão/CLIP/YOLO, AutoLabel/AutoTracker e Datasets.

### Signature Components
- **Environment Switcher (`env-switcher`):** Dropdown óptico na topbar com indicador de hardware ativo (Docker Local vs. Pod RunPod A100 vs. VPS L40S) e telemetria de VRAM/latência.
- **Editor BBox com Atalhos (`gallery-bbox-editor`):** Toolbar com atalhos de teclado (B = Box, V = Mover, H = Pan, 1-4 = Seleção de Classes), zoom de 50% a 250% e canvas interativo de alta taxa de quadros.

## Do's and Don'ts

### Do:
- **Do** manter rigorosamente a convenção de exatamente um botão primário esmeralda sólido por workspace.
- **Do** utilizar a fonte monoespaçada (`JetBrains Mono`) para todos os dados quantitativos, telemetria e coordenadas.
- **Do** respeitar o anel de foco `outline: 2px solid #10b981` com `outline-offset: 2px` para acessibilidade em todos os controles interativos.
- **Do** suportar `prefers-reduced-motion: reduce` desativando pulsos e animações de laser no canvas.
- **Do** aplicar o vidro óptico de 3 níveis através das classes `.glass-card`, `.glass-menu` e `.glass-modal`.

### Don't:
- **Don't** utilizar temas claros (Light Mode) — a interface é exclusivamente Dark-Only para fidelidade e ergonomia de visão computacional.
- **Don't** utilizar emojis como ícones de ação ou identificadores de categoria; utilize apenas ícones vetoriais monolínea de 1.7px.
- **Don't** aplicar sombras pretas opacas ou gradientes coloridos pesados em cards de fundo.
- **Don't** permitir que o texto de botões primários seja branco ou cinza claro; o botão primário esmeralda exige texto escuro de alto contraste (`text-zinc-950`).

## Iconografia Vetorial

Todos os ícones são construídos com linhas limpas, `strokeWidth="1.7"` e `viewBox="0 0 24 24"`, desenhados na escala de 14px a 16px (`w-3.5 h-3.5` ou `w-4 h-4`):

- **Hardware & Sistema:** `Cpu`, `Server`, `Zap`, `Activity`
- **IA & Modelos:** `Target` (YOLO / Detecção), `Sparkles` (Difusão), `Layers` (OpenCLIP / Camadas), `Wand` (AutoLabel / Invenção)
- **Dados & Anotação:** `Database`, `Folder`, `Tag`, `BoxSelect`, `Crosshair`, `Grid`, `List`
- **Controle de Treino:** `Play`, `Pause`, `Stop`, `Refresh`
- **Ferramentas de Canvas:** `ZoomIn`, `ZoomOut`, `Eye`, `FileText`, `Sliders`
- **Navegação & Utilidades:** `Search`, `Download`, `Plus`, `Trash`, `X`, `ChevronDown`, `Check`, `MoreVertical`, `Terminal`

## Anatomia de Componentes

### Topbar do Studio (`studio-topbar`)
- Altura: `h-14` (56px), fixada no topo (`sticky top-0 z-40`), com `bg-zinc-950/80 backdrop-blur-xl`.
- **Lado Esquerdo:** Identificador de produto com badge de versão em cápsula, separador vertical e **seletor de ambiente** (`env-switcher-btn`) com indicador pulsante de status.
- **Lado Direito:** Telemetria com badges de latência Rust Core (`0.2ms`), versão do motor Python (`PyTorch 2.4.1`), barra visual de uso de VRAM com barra de progresso colorida por estado (Cyan quando ocioso, Esmeralda quando treinando) e botão de configurações.

### Barra de Navegação Modular (`studio-tabs-bar`)
- Altura: `h-11` (44px), `bg-zinc-950 border-b border-zinc-800/80`.
- **Agrupamento Lógico com Separadores:**
  1. *Treino:* Difusão (`Flux·SDXL·1.5`), OpenCLIP (`Embedding`), YOLO (`v8/v9/v11`)
  2. *Preparo:* AutoLabel (`Difusão·CLIP`), AutoTracker (`Vídeo·Imagem`)
  3. *Dados:* Datasets (com contador dinâmico em badge mono)
- **Status Global à Direita:** Indicador de prontidão do daemon com bolinha animada (`animate-ping` durante execução).

### Workspaces (Layout de Divisão 2 Colunas)
- **Coluna de Configuração (Esquerda):** Largura fixa de 320px a 384px (`w-full md:w-80 lg:w-96`), rolagem independente, contendo seletores com dropdown óptico, inputs numéricos em grid 2 colunas, sliders de taxa de aprendizado e o CTA primário de treino.
- **Coluna de Monitoramento/Visualização (Direita):** Fluida, com padding `p-4 md:p-6`, contendo banner de status ativo, cards de métricas em grade, gráficos SVG de convergência e terminal de logs com rolagem.

### Editor de BBoxes da Galeria (`gallery-bbox-editor`)
- **Barra de Ferramentas:** Alternância entre Caixa (`B`), Mover/Selecionar (`V`) e Pan (`H`).
- **Paleta de Classes com Código de Atalho:**
  - `[1] solda_fria` → Verde Esmeralda (`bg-emerald-500`)
  - `[2] curto_circuito` → Âmbar (`bg-amber-500`)
  - `[3] componente_ausente` → Rosa/Vermelho (`bg-rose-500`)
  - `[4] trilha_rompida` → Ciano (`bg-cyan-500`)
- **Canvas com Zoom Flutuante:** Toolbar flutuante com zoom de 50% a 250% e botão de reset. Bounding boxes com coordenadas normalizadas (0 a 1), alças de redimensionamento nos cantos (`cursor-se-resize`), anel de foco `ring-2 ring-white/50` e etiqueta de identificação fixada no topo da caixa.

### Feedback: Toasts Flutuantes & Menus de Contexto
- **Toast (`studio-toast`):** Fixado no canto inferior direito (`bottom-5 right-5`), classe `.glass-menu` (valores de vidro em Elevation & Depth), com indicador colorido por tipo (`emerald-400` para sucesso, `rose-400` para erro, `cyan-400` para info) e entrada suave via `@keyframes fade-in`.
- **Menu de Contexto (`glass-context-menu`):** Posicionamento em coordenadas absolutas do clique direito (`top/left`), cantos arredondados (`rounded-2xl`), divisores sutis em `white/10` e suporte a ações por entidade (dataset, imagem de amostra ou configurações).

## Acessibilidade e Movimento

Foco visível: regra do anel de foco (`outline: 2px solid #10b981` com `outline-offset: 2px`) já definida em Do's and Don'ts — vale para `button`, `input`, `select`, `textarea` e `[role="tab"]` via `:focus-visible`. Abaixo, os detalhes exclusivos:

1. **Contraste em Estados Desabilitados:**
   - Redução de opacidade para `0.55` e `cursor: not-allowed` é aplicada estritamente aos controles que possuem o atributo `disabled`.
2. **Respeito a Preferências de Movimento (`prefers-reduced-motion`):**
   ```css
   @media (prefers-reduced-motion: reduce) {
     *, ::before, ::after {
       animation-duration: 0.01ms !important;
       animation-iteration-count: 1 !important;
       transition-duration: 0.01ms !important;
       scroll-behavior: auto !important;
     }
   }
   ```
   - Elimina os efeitos de pulso de treino (`pulse-glow`) e varredura a laser (`scanline`) para usuários sensíveis.

## Avaliação Crítica & Migração

### Pontos Fortes do Design System Atual
- **Fidelidade Visual Superior:** O protótipo transmite a sensação de um software de nível industrial e especializado, similar a ferramentas de alta engenharia (como Linear, Raycast ou interfaces de observabilidade Datadog/Grafana Dark).
- **Consistência de Superfícies:** A regra de 3 camadas de vidro óptico confere profundidade sem recorrer a sombras opacas pesadas.
- **Ergonomia dos Controles:** Os atalhos visuais (B/V/H, 1-4 para classes) e o layout de 2 colunas maximizam a velocidade de trabalho em telas ultrawide ou laptops convencionais.

### Oportunidades de Otimização na Migração (Next.js / TS)
1. **Componentização Modular:** O protótipo reúne ~40 estados em um único componente `App()`. Na base real, extrair os átomos (`Button`, `Badge`, `Select`, `Modal`, `Toast`, `GlassCard`) e moléculas (`BBoxCanvas`, `MetricCard`, `LossChart`).
2. **Renderizador de Canvas Real:** No HTML, as caixas são representadas por `div`s absolutas com zoom via transform/dimensões. Na aplicação de produção, utilizar `<canvas>` nativo (ou biblioteca leve como Konva/Fabric) para suportar milhares de polígonos e anotações complexas sem gargalo de nós no DOM.
3. **Métricas Reativas:** Substituir os gráficos SVG estáticos por componentes reativos com suporte a séries temporais em streaming (via WebSocket) conforme as épocas e steps são concluídos pelos runners de GPU.
