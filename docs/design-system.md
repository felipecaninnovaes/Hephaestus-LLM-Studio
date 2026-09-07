---
name: Hephaestus LLM Studio
description: Contrato de estilo v2 do redesign Arcane — UI dark violeta (brand #8350f2, oklch, Space Grotesk)
colors:
  primary: "#8350f2"
  primary-glow: "rgba(131, 80, 242, 0.25)"
  primary-hover: "#6c45bf"
  primary-subtle: "rgba(131, 80, 242, 0.08)"
  bg-base: "#0d0d0d"
  surface-card: "rgba(31, 27, 38, 0.70)"
  surface-menu: "rgba(25, 21, 32, 0.88)"
  surface-modal: "rgba(20, 17, 26, 0.94)"
  fg-default: "#f5f3f8"
  fg-muted: "rgba(245, 243, 248, 0.65)"
  border-default: "rgba(131, 80, 242, 0.14)"
  border-card: "rgba(131, 80, 242, 0.14)"
  border-top-glass: "rgba(255, 255, 255, 0.16)"
  status-success: "#34d399"
  status-alert: "#f59e0b"
  status-danger: "#ef4444"
  status-telemetry: "#06b6d4"
  tokens-oklch:
    bg: "oklch(15.9% 0.000 89.9)"
    surface: "oklch(23.2% 0.022 301.4)"
    surface-elevated: "oklch(28.5% 0.030 298.0)"
    fg: "oklch(96.0% 0.006 290.0)"
    muted: "oklch(65.0% 0.018 295.0)"
    border: "oklch(35.0% 0.045 295.0)"
    accent: "oklch(57.8% 0.229 292.2)"
    accent-hover: "oklch(50.1% 0.182 293.5)"
    accent-subtle: "oklch(41.0% 0.138 294.3)"
    accent-glow: "rgba(131, 80, 242, 0.25)"
  brand-scale:
    50: "#f5f3ff"
    100: "#ede9fe"
    200: "#ddd6fe"
    300: "#c4b5fd"
    400: "#a78bfa"
    500: "#8350f2"
    600: "#6c45bf"
    700: "#51358c"
    800: "#3b2569"
    900: "#261647"
    950: "#150a2b"
  zinc-scale:
    50: "#fbfaff"
    100: "#f4f2f9"
    200: "#e5e1ef"
    300: "#cfc9dc"
    400: "#9a92a6"
    500: "#756d82"
    600: "#585164"
    700: "#3e3749"
    800: "#2a2336"
    900: "#1f1b26"
    950: "#0d0d0d"
typography:
  display:
    fontFamily: "'Space Grotesk', -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.02em"
  headline:
    fontFamily: "'Space Grotesk', -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "-0.01em"
  title:
    fontFamily: "'Space Grotesk', -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: "0.875rem"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "normal"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Inter', sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.6
    letterSpacing: "normal"
  label:
    fontFamily: "'JetBrains Mono', 'IBM Plex Mono', ui-monospace, monospace"
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
responsive:
  sm: "640px"
  md: "768px"
  lg: "1024px"
  xl: "1280px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "#ffffff"
    rounded: "{rounded.md}"
    padding: "h-11 px-4 (lg, CTA único do contexto)"
  button-primary-hover:
    backgroundColor: "{colors.primary-hover}"
  button-secondary:
    backgroundColor: "transparent"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "h-9 px-3 (md, default das ações secundárias)"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "h-9 px-3 (md)"
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

> **Referência canônica única:** `temp_redesign/new_ui.html` (protótipo do redesign Arcane, 4153 linhas — fonte de verdade dos valores v2: `tailwind.config` linhas 16–62, tokens `:root` linhas 79–92, utilitários glass linhas 122–158), `apps/web/app/globals.css` (tokens vivos), `docs/frontend.md` (contratos §10).
>
> **Fonte de verdade de ESTILO para @frontend-dev e @ui-designer.**

## Overview

**Creative North Star: "The Arcane Foundry"**

O Hephaestus LLM Studio projeta a precisão de um laboratório de fundição e ótica industrial de ponta. É um ambiente de engenharia especializado em visão computacional e inteligência artificial generativa, concebido para operadores, pesquisadores e engenheiros de machine learning que passam horas refinando dados, rotulando mídias e monitorando jobs de GPU. Cada elemento na tela responde diretamente a uma necessidade funcional ou operacional; não há espaço para decorações gratuitas, gradientes genéricos ou emojis funcionais ("Anti-Slop").

A atmosfera visual é dominada pelo contraste rigoroso entre o fundo escuro profundo (`#0d0d0d`), superfícies translúcidas com textura óptica de vidro fumê em undertone berinjela e o pulsar arcano do acento Violeta (`#8350f2`). A densidade de dados é equilibrada por uma tipografia tripla: `Space Grotesk` para títulos e identidade, system sans (`-apple-system, 'SF Pro Text', 'Inter'`) para corpo e controles, e `JetBrains Mono` para todo dado numérico, telemetria, timestamp, log de terminal, paths ou rótulo de classe em caixa alta.

**Key Characteristics:**
- **Estética Dark-Only de Alta Fidelidade:** Protege contra fadiga ocular e destaca inspeções visuais em imagens e caixas delimitadoras (BBoxes).
- **Vidro Óptico em 3 Níveis:** Hierarquia dimensional transparente com desfoque gaussiano balanceado e bordas iluminadas, sem recurso a sombras pesadas e turvas.
- **Economia de Ação Estrita:** Exatamente um botão primário sólido violeta por painel de controle, orientando o foco imediato do usuário.
- **Microtipografia Técnica Exata:** Rótulos funcionais em tracking expandido (`letter-spacing: 0.08em`) e números mono-espaçados que não sofrem variação de largura tabular.

## Colors

Paleta dark-only profunda fundamentada em OKLCH com equivalente hexadecimal, priorizando contraste cirúrgico, distinção visual de estados e legibilidade de classes.

### Primary
- **Violeta Arcane** (`#8350f2` / `oklch(57.8% 0.229 292.2)`): Ação primária, confirmação de prontidão, anel de foco WCAG e pulso de execução ativo.
- **Violeta Hover** (`#6c45bf` / `oklch(50.1% 0.182 293.5)`): Estado hover do CTA primário.
- **Violeta Sutil** (`rgba(131, 80, 242, 0.08)` / `oklch(41.0% 0.138 294.3)`): Fundos de badge, trilhos e superfícies de ênfase leve.
- **Halo Violeta Glow** (`rgba(131, 80, 242, 0.25)`): Aura sutil em volta de elementos em execução ativa e indicadores de foco (`pulse-glow`: `0 0 15px → 0 0 28px rgba(131, 80, 242, …)`).

Escala brand completa (do `tailwind.config` do protótipo): 50 `#f5f3ff` · 100 `#ede9fe` · 200 `#ddd6fe` · 300 `#c4b5fd` · 400 `#a78bfa` · 500 `#8350f2` · 600 `#6c45bf` · 700 `#51358c` · 800 `#3b2569` · 900 `#261647` · 950 `#150a2b`.

### Secondary
- **Verde Sucesso / Ativo** (`#34d399`): Barras de progresso de treino e confirmações de status semântico.
- **Âmbar Pausa / Alerta** (`#f59e0b`): Ação de pausar treino e avisos de threshold.
- **Amarelo Runtime Python** (`#eab308` / `yellow-400`): Ponto de status do Motor PyTorch na telemetria.
- **Rosa Perigo / Abortar** (`#ef4444`): Ação de abortar ou deletar dataset e curvas de Loss.
- **Ciano Telemetria** (`#06b6d4`): Latência do Rust Core e selos AutoTracker.

### Neutral
- **Fundo Base** (`#0d0d0d` / `oklch(15.9% 0.000 89.9)`): Pano de fundo geral da viewport.
- **Superfície Card** (`rgba(31, 27, 38, 0.70)` / `oklch(23.2% 0.022 301.4)`): Cartões de datasets, cards de métricas e painéis de treino.
- **Superfície Menu & Topbar** (`rgba(25, 21, 32, 0.88)`): Dropdowns suspensos, toast e barra superior.
- **Superfície Modal** (`rgba(20, 17, 26, 0.94)` / `oklch(28.5% 0.030 298.0)`): Diálogos centrais de criação e confirmação.
- **Texto Principal** (`#f5f3f8` / `oklch(96.0% 0.006 290.0)`): Títulos, valores de destaque e rótulos de controle ativo.
- **Texto Secundário / Muted** (`rgba(245, 243, 248, 0.65)` / `oklch(65.0% 0.018 295.0)`): Labels explicativos, telemetria secundária e metadados.
- **Borda Estrutural** (`rgba(131, 80, 242, 0.14)` / `oklch(35.0% 0.045 295.0)`): Divisores, contornos de inputs e separadores de coluna.
- **Borda Superior do Vidro** (`rgba(255, 255, 255, 0.16)`): Topo iluminado das superfícies glass (ver regra Vidro Óptico).

Escala zinc do protótipo: 50 `#fbfaff` · 100 `#f4f2f9` · 200 `#e5e1ef` · 300 `#cfc9dc` · 400 `#9a92a6` · 500 `#756d82` · 600 `#585164` · 700 `#3e3749` · 800 `#2a2336` · 900 `#1f1b26` · 950 `#0d0d0d`.

### Named Rules
1. **The One CTA Rule.** Existe apenas um botão sólido violeta (`bg-brand-500`, `#8350f2`) visível como ação definitiva em cada painel (ex.: "Iniciar Treinamento", "Salvar Alterações"). Controles secundários utilizam acabamento ghost, vidro sutil ou contorno fino no novo estilo.
2. **The Brand-Only Rule.** A paleta do app é `brand-*`/`zinc-*`. Classes `emerald-*` são PROIBIDAS no código do app.
   > ⚠️ **BANNER OBRIGATÓRIO — armadilha de compatibilidade:** no protótipo (`temp_redesign/new_ui.html`, `tailwind.config`), a escala `emerald` foi REMAPEADA para a paleta violeta (`emerald-500 = #8350f2`) apenas por compatibilidade com classes antigas. No app real (Tailwind v4), `emerald-500 = #10b981` — o verde-esmeralda da v1! Qualquer classe `emerald-*` no código do app regressa o design à v1. Use sempre `brand-500` (`#8350f2`); trate ocorrências de `emerald-*` como bug de regressão visual.
3. **The Class Palette Integrity Rule (legado, escopo reduzido).** As cores semânticas de status (success `#34d399`, alert `#f59e0b`, danger `#ef4444`, telemetria `#06b6d4`) são reservadas e nunca reutilizadas para indicar estados de interface conflitantes na mesma área visual. As cores de classe de detecção do canvas seguem a mesma disciplina dentro do editor de BBoxes.
4. **The Monospace Truth Rule.** Todo número representando medição (VRAM, latência, tempo de epoch, dimensões em pixels, coordenadas de bounding box e loss), além de telemetria, paths e labels técnicos, deve ser renderizado obrigatoriamente em `JetBrains Mono` para garantir alinhamento tabular e evitar jittering visual durante atualizações ao vivo.
5. **The Vidro Óptico Rule (ex-Refractive Edge).** Toda superfície elevada de vidro possui borda superior (`border-top`) com opacidade de iluminação de 1.8x a 2.5x maior que as bordas laterais e inferiores, simulando reflexão ótica de luz zenital — com leve translucidez sobre o fundo escuro (ex.: `.glass-card` com `background: rgba(31, 27, 38, 0.70)` + `border-top: 1px solid rgba(255,255,255,0.16)` sobre borda base `rgba(131, 80, 242, 0.14)`). Ver os utilitários glass do protótipo como referência de contraste (`.glass-card` / `.glass-menu` / `.glass-modal`).
6. **The Anti-Scroll-Trap Rule.** Em `< md`, workspaces rolam como documento ÚNICO (painéis internos `overflow-visible`); scroll interno de coluna só existe em `≥ md` (`md:overflow-y-auto`) ou com `max-h` explícito + `overscroll-contain`. Nunca aninhe `overflow-y-auto` pai+filho em `flex-col` — no protótipo isso prendeu a imagem gerada do Playground fora do alcance (81px de scroll num conteúdo a 1010px).
7. **The Responsividade Rule.** Breakpoints Tailwind default (`sm 640 / md 768 / lg 1024 / xl 1280`). `< lg` = shell mobile: sidebar vira drawer (`min(85vw, 320px)`) + backdrop; títulos usam `truncate` com badge INLINE — nunca quebram em 2 linhas. Pílulas de sub-navegação = `overflow-x-auto` + fade edge na direita + auto-scroll posicionando a pílula ativa visível. Breadcrumb sempre em 1 linha (truncate no segmento do meio).
8. **The Densidade de Botões Rule.** Dois tamanhos canônicos: `md` = `h-9 px-3` (default, ações secundárias) e `lg` = `h-11 px-4` (SÓ o CTA primário do contexto). Hit area mínima 44×44 (WCAG 2.5.5 — o padding pode exceder o visual). Em `< md`, ações secundárias de toolbar (ex.: AutoLabel/AutoTracker/Exportar da galeria) colapsam para menu overflow "⋯" ou ícone com tooltip — nunca 4 botões de texto completo empilhando 2 linhas. Dropzone/imagens de preview com altura responsiva (`h-24` mobile, `h-36`+ desktop) — nunca altura fixa grande.
9. **The Truncamento Honesto Rule.** Placeholders, labels e valores longos usam `truncate`/`line-clamp` SEMPRE com `title=` (o texto completo permanece acessível). Pill de categoria/tag nunca corta texto sem affordance de continuação.

## Typography

**Display Font:** `Space Grotesk`, -apple-system, BlinkMacSystemFont, sans-serif — títulos e identidade  
**Body Font:** -apple-system, BlinkMacSystemFont, `SF Pro Text`, `Inter`, sans-serif — corpo e controles  
**Label/Mono Font:** `JetBrains Mono`, `IBM Plex Mono`, ui-monospace, monospace — labels, telemetria e valores numéricos  

**Character:** Pareamento de identidade geométrica (Space Grotesk) com sistema operacional neutro no corpo e rigor matemático absoluto da JetBrains Mono em telemetria, logs e anotações.

**Self-host (decisão da fatia redesign UI v2):** fontes servidas de `apps/web/fonts/*.woff2` (variable: Space Grotesk 300–700, JetBrains Mono 100–800) via `next/font/local` (`apps/web/app/layout.tsx`) — sem egress para `fonts.googleapis.com` no build. Motivo: `next/font/google` exigia egress no build, risco para o runner CI self-hosted.

### Hierarchy
- **Display** (SemiBold 600, 1.5rem / 24px, line-height 1.2, tracking -0.02em, Space Grotesk): Títulos principais de tela, nome da aplicação no shell e cabeçalhos de workspaces.
- **Headline** (SemiBold 600, 1.125rem / 18px, line-height 1.3, tracking -0.01em, Space Grotesk): Títulos de seções de workspace e nomes de datasets.
- **Title** (SemiBold 600, 0.875rem / 14px, line-height 1.4, Space Grotesk): Títulos de cards de parâmetros e labels principais de grupos de campos.
- **Body** (Regular 400, 0.875rem / 14px, line-height 1.6, system sans): Textos de descrição, explicações de status e instruções inline (comprimento máx. 65-75ch).
- **Label / Micro-Caps** (Medium 500, 0.6875rem / 11px, line-height 1.4, tracking 0.08em uppercase, JetBrains Mono): Rótulos de formulário, identificadores de classes, badges de status e telemetria.

## Layout

O estúdio opera em um modelo espacial de precisão, composto por:
- **Shell Global Fixo:** Topbar sticky (`h-14`, 56px) com indicador de ambiente e telemetria, sidebar macro de navegação (vira drawer `min(85vw, 320px)` + backdrop em `< lg`), seguida da barra de abas modular (`h-11`, 44px) agrupando Treino, Preparo e Dados.
- **Divisão de Workspace em 2 Colunas:**
  - *Coluna de Controle/Configuração:* Largura fixa entre 320px e 384px (`w-full md:w-80 lg:w-96`) com rolagem interna isolada apenas em `≥ md` (`md:overflow-y-auto`; em `< md` o workspace rola como documento único — ver regra Anti-Scroll-Trap). Concentra seletores, hiperparâmetros e o CTA principal.
  - *Coluna de Monitoramento/Anotação:* Painel fluido responsivo preenchendo o restante da viewport (`p-4 md:p-6`), contendo gráficos de convergência, canvas de imagem/vídeo ou terminal de streaming de logs.
- **Espaçamento e Ritmo:** Grade de 4px/8px. Gaps padrão de `12px` a `16px` entre cards de parâmetros e `24px` entre seções estruturais.

## Elevation & Depth

O sistema rejeita sombras difusas cinzentas ou pretas sólidas. A profundidade é produzida por **Vidro Óptico Translúcido (Frosted Glassmorphism)** em 3 níveis com refração de borda superior:

### Shadow & Glass Vocabulary
- **Nível 1 (.glass-card):** `background: rgba(31, 27, 38, 0.70)`, `backdrop-filter: blur(20px) saturate(160%)`, borda `1px solid rgba(131, 80, 242, 0.14)` com topo iluminado `border-top: 1px solid rgba(255,255,255,0.16)`, sombra `0 12px 30px -6px rgba(0,0,0,0.55)` + `inset 0 1px 0 rgba(255,255,255,0.06)`; hover eleva para `border-color: rgba(131, 80, 242, 0.30)` + `background: rgba(38, 33, 47, 0.78)`. Usado em cards de métricas, painéis de parâmetros e itens de grid de datasets.
- **Nível 2 (.glass-menu):** `background: rgba(25, 21, 32, 0.88)`, `backdrop-filter: blur(28px) saturate(190%)`, borda `1px solid rgba(131, 80, 242, 0.16)` com topo `rgba(255,255,255,0.22)` e sombra `0 24px 60px -8px rgba(0,0,0,0.85)` + `inset 0 1px 0 rgba(255,255,255,0.12)`. Usado em dropdowns suspensos, menus de contexto do botão direito e toasts flutuantes.
- **Nível 3 (.glass-modal):** `background: rgba(20, 17, 26, 0.94)`, `backdrop-filter: blur(32px) saturate(200%)`, borda `1px solid rgba(131, 80, 242, 0.20)` com topo `rgba(255,255,255,0.26)` e sombra `0 30px 80px -12px rgba(0,0,0,0.90)` + `inset 0 1px 0 rgba(255,255,255,0.15)`. Usado em caixas de diálogo centrais com backdrop escurecido.

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
- **Primary (CTA único, `lg`):** `bg-brand-500 (#8350f2) text-white h-11 px-4 text-sm font-medium rounded-lg hover:bg-brand-600 transition-colors shadow-sm` + glow `rgba(131, 80, 242, 0.25)` em execução ativa.
  - **Nota de acessibilidade (fatia redesign UI v2):** CTA em `brand-500` (`#8350f2`) + `text-white` ≈ 4.78:1 — passa WCAG AA (texto normal, 4.5:1). Este é o piso: não clarear o fundo do CTA; `brand-600+` só aumenta contraste.
- **Secondary / Ghost (`md`, default):** `bg-zinc-900/60 border border-zinc-700/80 text-zinc-200 h-9 px-3 text-xs rounded-lg hover:bg-zinc-800`.
- **Destructive / Abort:** `border border-rose-500/60 text-rose-400 bg-rose-950/20 h-9 px-3 text-xs rounded-lg hover:bg-rose-950/40`.
- **Warning / Pause:** `border border-amber-500/60 text-amber-400 bg-amber-950/20 h-9 px-3 text-xs rounded-lg hover:bg-amber-950/40`.

### Chips & Badges
- **Status Badge:** Cápsula pill com borda fina translúcida e ponto luminoso pulsante (`w-1.5 h-1.5 rounded-full bg-brand-400 animate-pulse`).
- **Telemetry Capsule:** `bg-black/40 border border-white/10 px-2.5 py-1 text-zinc-300 font-mono text-[11px] rounded-full`.
- **Brand Badge (studio-badge):** `font-mono text-[11px] uppercase tracking-[0.08em] text-brand-500 (#8350f2) border border-brand-500/35 bg-brand-500/10 rounded-full px-3 py-1`.

### Cards / Containers
- Base `.glass-card` com preenchimento interno padronizado (`p-4` a `p-6`), cabeçalho de título com tracking sutil e separador `border-b border-white/5` opcional.

### Inputs & Fields
- `bg-black/40 border border-zinc-800 text-zinc-100 rounded-lg px-3 py-1.5 text-xs font-mono focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500`. Em `< md` (iOS Safari), inputs/selects/textarea usam `font-size: 16px` para prevenir zoom automático.

### Navigation
- Topbar sticky com seletor de pods em dropdown; sidebar macro (drawer em `< lg`); barra de abas com divisores verticais sutis entre os grupos de Difusão/CLIP/YOLO, AutoLabel/AutoTracker e Datasets; pílulas de sub-navegação com `overflow-x-auto` + fade edge na direita + auto-scroll da pílula ativa.

### Signature Components
- **Environment Switcher (`env-switcher`):** Dropdown óptico na topbar com indicador de hardware ativo (Docker Local vs. Pod RunPod A100 vs. VPS L40S) e telemetria de VRAM/latência.
- **Editor BBox com Atalhos (`gallery-bbox-editor`):** Toolbar com atalhos de teclado (B = Box, V = Mover, H = Pan, 1-4 = Seleção de Classes), zoom de 50% a 250% e canvas interativo de alta taxa de quadros.

## Do's and Don'ts

### Do:
- **Do** manter rigorosamente a convenção de exatamente um botão primário violeta sólido (`bg-brand-500 #8350f2`) por workspace.
- **Do** utilizar a fonte monoespaçada (`JetBrains Mono`) para todos os dados quantitativos, telemetria, paths e coordenadas.
- **Do** respeitar o anel de foco `outline: 2px solid #8350f2` com `outline-offset: 2px` para acessibilidade em todos os controles interativos.
- **Do** suportar `prefers-reduced-motion: reduce` desativando pulsos e animações de laser no canvas.
- **Do** aplicar o vidro óptico de 3 níveis através das classes `.glass-card`, `.glass-menu` e `.glass-modal`.
- **Do** aplicar fade edge na direita de trilhos horizontais scrolláveis (pílulas de sub-navegação).
- **Do** usar `truncate` + `title=` em placeholders, labels e valores longos (truncamento honesto).

### Don't:
- **Don't** utilizar temas claros (Light Mode) — a interface é exclusivamente Dark-Only para fidelidade e ergonomia de visão computacional.
- **Don't** utilizar emojis como ícones de ação ou identificadores de categoria; utilize apenas ícones vetoriais monolínea de 1.7px.
- **Don't** aplicar sombras pretas opacas ou gradientes coloridos pesados em cards de fundo.
- **Don't** permitir que o texto do botão primário violeta seja de baixo contraste; o CTA usa texto claro sobre `#8350f2`.
- **Don't** usar classes `emerald-*` no código do app — elas resolvem para o verde `#10b981` da v1 no Tailwind v4 (ver banner da regra Brand-Only).
- **Don't** aninhar `overflow-y-auto` pai+filho em `flex-col` (scroll-trap — ver regra Anti-Scroll-Trap).
- **Don't** usar botão de texto completo em toolbar mobile — colapsar para menu overflow "⋯" ou ícone com tooltip.
- **Don't** quebrar título da sidebar em 2 linhas — usar `truncate` com badge INLINE.

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
- **Lado Esquerdo:** Identificador de produto (Space Grotesk) com badge de versão em cápsula, separador vertical e **seletor de ambiente** (`env-switcher-btn`) com indicador pulsante de status.
- **Lado Direito:** Telemetria em JetBrains Mono com badges de latência Rust Core, versão do motor Python (`PyTorch 2.4.1`), barra visual de uso de VRAM com barra de progresso colorida por estado e botão de configurações.

### Barra de Navegação Modular (`studio-tabs-bar`)
- Altura: `h-11` (44px), `bg-zinc-950 border-b border-zinc-800/80`.
- **Agrupamento Lógico com Separadores:**
  1. *Treino:* Difusão (`Flux·SDXL·1.5`), OpenCLIP (`Embedding`), YOLO (`v8/v9/v11`)
  2. *Preparo:* AutoLabel (`Difusão·CLIP`), AutoTracker (`Vídeo·Imagem`)
  3. *Dados:* Datasets (com contador dinâmico em badge mono)
- **Status Global à Direita:** Indicador de prontidão do daemon com bolinha animada (`animate-ping` durante execução).

### Workspaces (Layout de Divisão 2 Colunas)
- **Coluna de Configuração (Esquerda):** Largura fixa de 320px a 384px (`w-full md:w-80 lg:w-96`), rolagem independente apenas em `≥ md`; em `< md` o workspace rola como documento único (ver regra Anti-Scroll-Trap). Contém seletores com dropdown óptico, inputs numéricos em grid 2 colunas, sliders de taxa de aprendizado e o CTA primário de treino (`h-11`).
- **Coluna de Monitoramento/Visualização (Direita):** Fluida, com padding `p-4 md:p-6`, contendo banner de status ativo, cards de métricas em grade, gráficos SVG de convergência e terminal de logs com rolagem.
- **Mobile (`< lg`):** Sidebar vira drawer + backdrop; breadcrumb em 1 linha com truncate no segmento do meio; pílulas de sub-navegação com scroll horizontal + fade edge.

### Editor de BBoxes da Galeria (`gallery-bbox-editor`)
- **Barra de Ferramentas:** Alternância entre Caixa (`B`), Mover/Selecionar (`V`) e Pan (`H`). Em `< md`, ações secundárias colapsam para overflow "⋯" (ver regra Densidade de Botões).
- **Paleta de Classes com Código de Atalho:**
  - `[1] solda_fria` → Violeta brand (`bg-brand-500 #8350f2`)
  - `[2] curto_circuito` → Âmbar (`bg-amber-500`)
  - `[3] componente_ausente` → Rosa/Vermelho (`bg-rose-500`)
  - `[4] trilha_rompida` → Ciano (`bg-cyan-500`)
- **Canvas com Zoom Flutuante:** Toolbar flutuante com zoom de 50% a 250% e botão de reset. Bounding boxes com coordenadas normalizadas (0 a 1), alças de redimensionamento nos cantos (`cursor-se-resize`), anel de foco `ring-2 ring-white/50` e etiqueta de identificação fixada no topo da caixa.

### Feedback: Toasts Flutuantes & Menus de Contexto
- **Toast (`studio-toast`):** Fixado no canto inferior direito (`bottom-5 right-5`), classe `.glass-menu` (valores de vidro em Elevation & Depth), com indicador colorido por tipo (`brand-400` para sucesso/info violeta, `rose-400` para erro, `cyan-400` para telemetria) e entrada suave via `@keyframes fade-in`.
- **Menu de Contexto (`glass-context-menu`):** Posicionamento em coordenadas absolutas do clique direito (`top/left`), cantos arredondados (`rounded-2xl`), divisores sutis em `white/10` e suporte a ações por entidade (dataset, imagem de amostra ou configurações).

## Acessibilidade e Movimento

Foco visível: regra do anel de foco (`outline: 2px solid #8350f2` com `outline-offset: 2px`) já definida em Do's and Don'ts — vale para `button`, `input`, `select`, `textarea` e `[role="tab"]` via `:focus-visible`. Abaixo, os detalhes exclusivos:

1. **Contraste em Estados Desabilitados:**
   - Redução de opacidade para `0.55` e `cursor: not-allowed` é aplicada estritamente aos controles que possuem o atributo `disabled`.
2. **Hit Area Mínima (WCAG 2.5.5):**
   - Alvos de toque com `min-height: 44px` / `min-width: 44px` (`.touch-target`); o padding pode exceder o visual do botão. Sliders com thumb de 22px + borda branca para aderência em touch (`touch-action: manipulation` / `pan-y`).
   - Safe areas iOS/Android (`.safe-area-top` / `.safe-area-bottom`) e `font-size: 16px` em inputs mobile para prevenir zoom automático do Safari.
3. **Respeito a Preferências de Movimento (`prefers-reduced-motion`):**
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
1. **Componentização Modular:** O protótipo reúne dezenas de estados em componentes React monolíticos. Na base real, extrair os átomos (`Button` md/lg, `Badge`, `Select`, `Modal`, `Toast`, `GlassCard`) e moléculas (`BBoxCanvas`, `MetricCard`, `LossChart`).
2. **Renderizador de Canvas Real:** No protótipo, as caixas são representadas por `div`s absolutas com zoom via transform/dimensões. Na aplicação de produção, utilizar `<canvas>` nativo (ou biblioteca leve como Konva/Fabric) para suportar milhares de polígonos e anotações complexas sem gargalo de nós no DOM.
3. **Métricas Reativas:** Substituir os gráficos SVG estáticos por componentes reativos com suporte a séries temporais em streaming (via WebSocket) conforme as épocas e steps são concluídos pelos runners de GPU.

## Versionamento

- `v2 — 2026-09-07` — redesign Arcane do usuário (protótipo `temp_redesign/new_ui.html`, paleta violeta oklch, Space Grotesk, sidebar macro).
- `v1 — esmeralda/Inter` (protótipo `ai-vision-training-studio.html`, DEPRECADO nesta fatia).
