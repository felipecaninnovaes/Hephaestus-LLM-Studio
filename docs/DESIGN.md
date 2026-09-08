---
name: Hephaestus LLM Studio
description: Estúdio de visão computacional, curadoria de datasets e orquestração de treino (UI Dark Arcane)
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
  status-runtime-python: "#eab308"
  brand-50: "#f5f3ff"
  brand-100: "#ede9fe"
  brand-200: "#ddd6fe"
  brand-300: "#c4b5fd"
  brand-400: "#a78bfa"
  brand-500: "#8350f2"
  brand-600: "#6c45bf"
  brand-700: "#51358c"
  brand-800: "#3b2569"
  brand-900: "#261647"
  brand-950: "#150a2b"
  zinc-50: "#fbfaff"
  zinc-100: "#f4f2f9"
  zinc-200: "#e5e1ef"
  zinc-300: "#cfc9dc"
  zinc-400: "#9a92a6"
  zinc-500: "#756d82"
  zinc-600: "#585164"
  zinc-700: "#3e3749"
  zinc-800: "#2a2336"
  zinc-900: "#1f1b26"
  zinc-950: "#0d0d0d"
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
  2xl: "20px"
  full: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
components:
  button-primary:
    backgroundColor: "rgba(131, 80, 242, 0.12)"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-primary-hover:
    backgroundColor: "rgba(131, 80, 242, 0.18)"
  button-secondary:
    backgroundColor: "rgba(255, 255, 255, 0.05)"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.fg-muted}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
    height: "36px"
  button-destructive:
    backgroundColor: "rgba(239, 68, 68, 0.12)"
    textColor: "{colors.fg-default}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
    height: "36px"
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

## Overview

**Creative North Star: "The Arcane Foundry"**

O Hephaestus LLM Studio projeta a precisão de um laboratório de fundição e ótica industrial de ponta. É um ambiente de engenharia especializado em visão computacional e inteligência artificial generativa, concebido para operadores, pesquisadores e engenheiros de machine learning que passam horas refinando dados, rotulando mídias e monitorando jobs de GPU. Cada elemento na tela responde diretamente a uma necessidade funcional ou operacional; não há espaço para decorações gratuitas, gradientes genéricos ou emojis funcionais ("Anti-Slop").

A atmosfera visual é dominada pelo contraste rigoroso entre o fundo escuro profundo (`#0d0d0d`), superfícies translúcidas com textura óptica de vidro fumê em undertone berinjela e o pulsar arcano do acento Violeta (`#8350f2` / `brand-500`). A densidade de dados é equilibrada por uma tipografia tripla: `Space Grotesk` para títulos e identidade, system sans (`-apple-system, 'SF Pro Text', 'Inter'`) para corpo e controles, e `JetBrains Mono` para todo dado numérico, telemetria, timestamp, log de terminal, paths ou rótulo de classe em caixa alta.

**Key Characteristics:**
- **Estética Dark-Only de Alta Fidelidade:** Protege contra fadiga ocular e destaca inspeções visuais em imagens e caixas delimitadoras (BBoxes).
- **Vidro Óptico em 3 Níveis:** Hierarquia dimensional transparente com desfoque gaussiano balanceado e bordas iluminadas, sem sombras pesadas ou turvas.
- **Economia de Ação Estrita:** Exatamente um CTA primário translúcido por painel de controle, orientando o foco imediato do operador.
- **Microtipografia Técnica Exata:** Rótulos funcionais em tracking expandido (`letter-spacing: 0.08em`) e números monoespaçados com alinhamento tabular absoluto.
- **Densidade Compacta Arcane:** `html { font-size: 14px }` com hit-areas otimizadas (28px a 35px mínimo WCAG 2.5.8), gerando compacidade e ergonomia profissional.

## Colors

Paleta dark-only profunda fundamentada em OKLCH com equivalente hexadecimal, priorizando contraste cirúrgico, distinção visual de estados e legibilidade de classes de detecção.

### Primary
- **Violeta Arcane** (`#8350f2` / `oklch(57.8% 0.229 292.2)` / `brand-500`): Ação primária contextual, confirmação de prontidão, anel de foco WCAG e pulso de execução ativo.
- **Violeta Hover** (`#6c45bf` / `oklch(50.1% 0.182 293.5)` / `brand-600`): Estado hover do CTA primário.
- **Violeta Sutil** (`rgba(131, 80, 242, 0.08)` / `oklch(41.0% 0.138 294.3)`): Fundos de badges de estado, trilhos e superfícies de ênfase leve.
- **Halo Violeta Glow** (`rgba(131, 80, 242, 0.25)`): Aura sutil ao redor de elementos em execução ativa e indicadores de foco.

Escala brand completa:
- `brand-50`: `#f5f3ff` · `brand-100`: `#ede9fe` · `brand-200`: `#ddd6fe` · `brand-300`: `#c4b5fd` · `brand-400`: `#a78bfa`
- `brand-500`: `#8350f2` (marca primária) · `brand-600`: `#6c45bf` · `brand-700`: `#51358c` · `brand-800`: `#3b2569`
- `brand-900`: `#261647` · `brand-950`: `#150a2b`

### Secondary
- **Verde Sucesso / Ativo** (`#34d399`): Barras de progresso de treino, classe 1 de solda/inspeção (`solda_fria`) e confirmações de status concluído.
- **Âmbar Alerta / Pausa** (`#f59e0b`): Ação de pausar treino, avisos de threshold e classe 2 de anotação (`curto_circuito`).
- **Amarelo Runtime Python** (`#eab308`): Ponto de status do runtime Python/PyTorch na telemetria.
- **Rosa Perigo / Abortar** (`#ef4444`): Ação de abortar job, exclusão de dados e classe 3 de anotação (`componente_ausente`).
- **Ciano Telemetria** (`#06b6d4`): Latência do Rust Core, selos AutoTracker e classe 4 de anotação (`trilha_rompida`).

### Neutral
- **Fundo Base** (`#0d0d0d` / `oklch(15.9% 0.000 89.9)`): Pano de fundo geral da viewport.
- **Superfície Card** (`rgba(31, 27, 38, 0.70)` / `oklch(23.2% 0.022 301.4)`): Cartões de datasets, cards de métricas e painéis de treino.
- **Superfície Menu & Topbar** (`rgba(25, 21, 32, 0.88)`): Dropdowns suspensos, toasts e cabeçalho de navegação.
- **Superfície Modal** (`rgba(20, 17, 26, 0.94)` / `oklch(28.5% 0.030 298.0)`): Diálogos centrais de criação e confirmação.
- **Texto Principal** (`#f5f3f8` / `oklch(96.0% 0.006 290.0)`): Títulos, valores de destaque e rótulos de controle ativo.
- **Texto Secundário / Muted** (`rgba(245, 243, 248, 0.65)` / `oklch(65.0% 0.018 295.0)`): Labels explicativos, telemetria secundária e metadados.
- **Borda Estrutural** (`rgba(131, 80, 242, 0.14)` / `oklch(35.0% 0.045 295.0)`): Divisores, contornos de inputs e separadores de coluna.
- **Borda Superior do Vidro** (`rgba(255, 255, 255, 0.16)`): Topo iluminado das superfícies glass reflexivas.

Escala zinc completa (undertone berinjela neutro):
- `zinc-50`: `#fbfaff` · `zinc-100`: `#f4f2f9` · `zinc-200`: `#e5e1ef` · `zinc-300`: `#cfc9dc` · `zinc-400`: `#9a92a6`
- `zinc-500`: `#756d82` · `zinc-600`: `#585164` · `zinc-700`: `#3e3749` · `zinc-800`: `#2a2336`
- `zinc-900`: `#1f1b26` · `zinc-950`: `#0d0d0d`

### Named Rules
**The One CTA Rule.** Existe apenas um CTA definitivo por contexto, no estilo outline-violeta translúcido (`border-brand-500/30` + `bg-brand-500/[0.12]`, texto `text-white` com inset highlight superior `rgba(255,255,255,0.08)`); NUNCA violeta sólido (`bg-brand-500`) como fundo de botão. O tom sólido `bg-brand-500` fica RESERVADO exclusivamente a elementos de destaque que não sejam botão (ex.: barra lateral ativa da Sidebar, badges de estado se necessário). Controles secundários utilizam o secundário translúcido (`border-white/10 bg-white/[0.05]`) ou ghost.

**The Brand-Only Rule.** A paleta do app é estritamente `brand-*` e `zinc-*`. Classes utilitárias `emerald-*` são TERMINANTEMENTE PROIBIDAS no código da aplicação. No Tailwind v4 nativo, `emerald-500` resolve para o verde da versão inicial legada (`#10b981`). Qualquer classe `emerald-*` no código constitui bug de regressão visual. Estados de sucesso usam a cor semântica literal `#34d399`.

**The Class Palette Integrity Rule.** As cores semânticas de status (sucesso `#34d399`, alerta `#f59e0b`, perigo `#ef4444`, telemetria `#06b6d4`, runtime `#eab308`) são reservadas e nunca reutilizadas para indicar estados de interface conflitantes na mesma área visual. As cores de classes de detecção de BBoxes seguem mapeamento rigoroso e consistente.

## Typography

**Display Font:** `Space Grotesk`, -apple-system, BlinkMacSystemFont, sans-serif (títulos principais e identidade do estúdio)  
**Body Font:** -apple-system, BlinkMacSystemFont, `SF Pro Text`, `Inter`, sans-serif (corpo, descrições e controles de UI)  
**Label/Mono Font:** `JetBrains Mono`, `IBM Plex Mono`, ui-monospace, monospace (labels, telemetria, logs e métricas)  

**Character:** Casamento entre a geometria impositiva de `Space Grotesk` nos títulos, a neutralidade ergonômica da sans de sistema no corpo e o rigor matemático tabular de `JetBrains Mono` em toda telemetria e anotação técnica.

**Self-Hosting:** Fontes servidas localmente de `apps/web/fonts/*.woff2` (Space Grotesk 300–700 variable, JetBrains Mono 100–800 variable) via `next/font/local` (`apps/web/app/layout.tsx`), eliminando egress externo para Google Fonts e garantindo compilação offline e em CI hermético.

### Hierarchy
- **Display** (SemiBold 600, 1.5rem / 21px efetivos no root 14px, line-height 1.2, tracking -0.02em, Space Grotesk): Títulos principais de tela, nome da aplicação no shell e cabeçalhos de workspaces.
- **Headline** (SemiBold 600, 1.125rem / 15.75px efetivos, line-height 1.3, tracking -0.01em, Space Grotesk): Títulos de seções de workspace e nomes de datasets.
- **Title** (SemiBold 600, 0.875rem / 12.25px efetivos, line-height 1.4, Space Grotesk): Títulos de cards de parâmetros e labels de grupos de campos.
- **Body** (Regular 400, 0.875rem / 12.25px efetivos, line-height 1.6, system sans): Textos de descrição, explicações de status e instruções inline (comprimento máx. 65-75ch).
- **Label / Micro-Caps** (Medium 500, 0.6875rem / 9.6px efetivos, line-height 1.4, tracking 0.08em uppercase, JetBrains Mono): Rótulos de formulário, identificadores de classes, badges de status, coordenadas e telemetria.

### Named Rules
**The Monospace Truth Rule.** Todo número representando medição (VRAM, CPU, RAM, latência, tempo de epoch, dimensões em pixels, coordenadas de bounding box e loss), além de telemetria, caminhos de arquivo (paths) e labels técnicos, deve ser renderizado obrigatoriamente em `JetBrains Mono` para garantir alinhamento tabular e evitar oscilações visuais (jittering) durante atualizações ao vivo.

## Layout

O estúdio opera em um modelo espacial de precisão profissional, composto por:
- **Shell Global Fixo:**
  - *Sidebar Macro:* Navegação primária vertical fixa à esquerda com módulos de sistema ("Dados & Anotação" `/datasets*`, "Forja & Treinamento" `/jobs`, "Execução & Playground", "Configurações"), card de telemetria de hardware ao vivo (VRAM, CPU, RAM, GPUs ativas) e botão de encerramento de sessão (`POST /api/auth/logout`).
  - *Header Global:* Breadcrumbs em 1 linha com truncate no segmento central e chip "Local" de indicação de nó ativo.
- **Divisão de Workspace em 2 Colunas:**
  - *Coluna de Controle/Configuração:* Largura fixa entre 320px e 384px (`w-full md:w-80 lg:w-96`) com rolagem interna isolada apenas em telas desktop (`md:overflow-y-auto`). Concentra seletores, formulários de treino/preparo e o CTA principal.
  - *Coluna de Monitoramento/Visualização:* Painel fluido responsivo preenchendo o restante da viewport (`p-4 md:p-6`), contendo histórico de atividade, gráficos de métricas, canvas de imagem/vídeo ou terminal de streaming.
- **Espaçamento e Ritmo:** Grade estrita de 4px/8px. Gaps padrão de 12px a 16px entre cards de parâmetros e 24px entre seções estruturais.

### Densidade Global (Fidelidade Arcane v2.10.1)
- `html { font-size: 14px }`: Densidade compacta de ferramentas profissionais; todos os valores expressos em `rem` encolhem uniformemente ~12,5%.
- Tabela de tamanhos de botão (root 14px):
  - `sm`: `h-8 px-3 rounded-md` (ações compactas em toolbars densas)
  - `default`: `h-9 px-4` (com ícone: `px-3`) (padrão de ações secundárias e filtros)
  - `lg`: `h-10 px-5` (CTA primário do contexto)

### Named Rules
**The Anti-Scroll-Trap Rule.** Em telas menores (`< md`), workspaces rolam como um documento ÚNICO (painéis internos `overflow-visible`). Rolagem interna de coluna só é permitida em telas médias ou superiores (`≥ md` com `md:overflow-y-auto`) ou com `max-h` explícito acompanhado de `overscroll-contain`. É terminantemente proibido aninhar `overflow-y-auto` pai e filho em containers `flex-col`, prevenindo bloqueio de scroll de visualizações em viewport móvel.

**The Responsividade Rule.** Breakpoints Tailwind padrão (`sm 640 / md 768 / lg 1024 / xl 1280`). Em telas `< lg`, a Sidebar colapsa automaticamente em Drawer lateral retrátil (`w-[min(85vw,320px)]`) com backdrop escuro translúcido (`bg-black/70 backdrop-blur-sm`), fechando com `Escape` ou clique fora. Títulos utilizam `truncate` com badge inline (nunca quebram em duas linhas). Pílulas de sub-navegação utilizam `overflow-x-auto` com fade edge suave à direita e auto-scroll posicionando a pílula ativa.

**The Densidade de Botões Rule.** Botões seguem rigorosamente os três tamanhos canônicos (`sm h-8`, `default h-9`, `lg h-10`). A hit-area mínima em desktop e tablet é de 28px (conforme WCAG 2.5.8), priorizando fidelidade visual compacta. Em visualizações móveis (`< md`), barras de ações secundárias com múltiplos botões colapsam para menu dropdown overflow ("⋯") ou ícones com tooltip, impedindo empilhamento em múltiplas linhas.

**The Truncamento Honesto Rule.** Placeholders, labels de arquivos, slugs e valores longos utilizam `truncate` ou `line-clamp` SEMPRE acompanhados do atributo HTML `title="..."` contendo o texto completo e fidedigno, garantindo acessibilidade e auditoria sem truncamento destrutivo.

## Elevation & Depth

O sistema rejeita sombras difusas cinzentas ou pretas sólidas. A profundidade é produzida por **Vidro Óptico Translúcido (Frosted Glassmorphism)** em 3 níveis com refração de borda superior zenital e undertone berinjela:

### Shadow & Glass Vocabulary
- **Nível 1 (.glass-card):** `background: rgba(31, 27, 38, 0.70)`, `backdrop-filter: blur(20px) saturate(160%)`, borda `1px solid rgba(131, 80, 242, 0.14)` com topo iluminado `border-top: 1px solid rgba(255, 255, 255, 0.16)`, sombra `0 12px 30px -6px rgba(0, 0, 0, 0.55)` + `inset 0 1px 0 rgba(255, 255, 255, 0.06)`. Hover eleva para `border-color: rgba(131, 80, 242, 0.30)` e `background: rgba(38, 33, 47, 0.78)`. Usado em cards de métricas, painéis de parâmetros e itens de grid de datasets.
- **Nível 2 (.glass-menu):** `background: rgba(25, 21, 32, 0.88)`, `backdrop-filter: blur(28px) saturate(190%)`, borda `1px solid rgba(131, 80, 242, 0.16)` com topo `rgba(255, 255, 255, 0.22)`, sombra `0 24px 60px -8px rgba(0, 0, 0, 0.85)` + `inset 0 1px 0 rgba(255, 255, 255, 0.12)`. Usado em dropdowns suspensos, menus de contexto e toasts flutuantes.
- **Nível 3 (.glass-modal):** `background: rgba(20, 17, 26, 0.94)`, `backdrop-filter: blur(32px) saturate(200%)`, borda `1px solid rgba(131, 80, 242, 0.20)` com topo `rgba(255, 255, 255, 0.26)`, sombra `0 30px 80px -12px rgba(0, 0, 0, 0.90)` + `inset 0 1px 0 rgba(255, 255, 255, 0.15)`. Usado em diálogos centrais com backdrop escurecido.

### Named Rules
**The Vidro Óptico Rule.** Toda superfície elevada de vidro possui sua borda superior (`border-top`) com opacidade de iluminação de 1.8x a 2.5x maior que as bordas laterais e inferiores, simulando reflexão ótica de luz zenital em uma bancada de precisão industrial.

## Shapes

- **Bordas e Cantos:**
  - `rounded-sm` (6px): Badges mono, microtags, labels de coordenadas BBox no canvas.
  - `rounded-md` (8px): Botões de ação secundária (`sm`), inputs de formulário, switches e selects.
  - `rounded-lg` (12px): Botões padrão (`h-9`), CTA primário (`lg h-10`), pílulas de navegação.
  - `rounded-xl` (16px): Cards de métricas, blocos de parâmetros e painéis laterais.
  - `rounded-2xl` (20px): Menus suspensos, modais de diálogo e painel de autenticação.
  - `rounded-full` (9999px): Indicadores de status pulsante, segmented controls e chips de filtros.
- **Canvas BBox:** Caixas delimitadoras com borda sólida de 1.5px na cor da classe correspondente, alças quadradas de redimensionamento nos vértices (`w-2 h-2`) com cursor direcional adequado, anel de foco `ring-2 ring-white/50` e tag identificadora fixada no vértice superior esquerdo.

## Components

### Buttons
Base comum a todos os botões (aplicar sempre antes de tom/tamanho):
```
inline-flex items-center justify-center gap-2 text-sm font-medium whitespace-nowrap select-none transition active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--bg)] [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55
```

- **Primary (CTA único, outline-violeta translúcido):** `rounded-lg border border-brand-500/30 bg-brand-500/[0.12] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-brand-500/50 hover:bg-brand-500/[0.18] active:scale-[0.985]`. Tamanho `lg h-10 px-5` quando CTA principal de tela/painel.
- **Secondary (Default):** `rounded-lg border border-white/10 bg-white/[0.05] text-zinc-100 shadow-[inset_0_1px_0_rgba(255,255,255,0.06),0_1px_2px_rgba(0,0,0,0.16)] hover:border-white/20 hover:bg-white/[0.10]`. Tamanho `h-9 px-4` (com ícone: `px-3`).
- **Ghost:** `bg-transparent border-transparent text-zinc-300 hover:bg-white/[0.06] hover:text-white`. Tamanho `h-9 px-3`.
- **Primary Destructive:** `rounded-lg border border-[#ef4444]/30 bg-[#ef4444]/[0.12] text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.08),0_1px_2px_rgba(0,0,0,0.18)] hover:border-[#ef4444]/50 hover:bg-[#ef4444]/[0.18]`. Usado para exclusões definitivas e abortos forçados de job.
- **Segmented Control (Toggle grade/lista e afins):** Container `inline-flex rounded-full border border-white/10 bg-black/40 p-1`; item `h-7 px-2.5 rounded-full [&_svg]:size-4`; item ativo `rounded-full bg-brand-500/[0.18] text-brand-300`; item inativo `text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.05]`.
- **Pílula de Submódulo / Abas:** Altura `h-9`, texto `text-sm`; ativa `rounded-lg border border-brand-500/30 bg-brand-500/[0.12] text-white` com ícone `brand-400`; inativa `rounded-lg border border-white/8 bg-white/[0.03] text-zinc-400 hover:text-zinc-200` com ícone `zinc-500`.

### Chips & Badges
- **Status Badge:** Cápsula pill com borda translúcida fina e ponto luminoso pulsante (`w-1.5 h-1.5 rounded-full bg-brand-400 animate-pulse`).
- **Telemetry Capsule:** `bg-black/40 border border-white/10 px-2.5 py-1 text-zinc-300 font-mono text-[11px] rounded-full`.
- **Brand Badge (.studio-badge):** `font-mono text-[11px] uppercase tracking-[0.08em] text-brand-500 border border-brand-500/35 bg-brand-500/10 rounded-full px-3 py-1`.

### Cards & Containers
- Base `.glass-card` com preenchimento padronizado (`p-4` a `p-6`), cabeçalho com tracking e separador `border-b border-white/5` opcional.

### Inputs & Fields
- `bg-black/40 border border-zinc-800 text-zinc-100 rounded-lg px-3 py-1.5 text-xs font-mono focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500`. Em visualizações móveis (`< md`), inputs e textareas adotam `font-size: 16px` para prevenir zoom automático no Safari iOS.

### Navigation
- Sidebar macro à esquerda com módulos de sistema, drawer mobile retrátil em `< lg`, breadcrumbs em linha única e pílulas com fade edge.

### Login — AuthAmbient (Fidelidade Arcane v2.10.1)
Fundo fixo com 4 camadas óticas:
1. **mesh:** 4 gradientes radiais violeta (`rgba(131, 80, 242, 0.14)` em 18%/22%, 10% em 82%/78%, 8% em 78%/18%, 6% em 22%/82%, opacidade 0.5).
2. **grid:** Padrão SVG 48×48px (linhas 1px cinza `#969696` com opacidade 0.15) com máscara radial central (`mask-image: radial-gradient(circle at 50% 50%, #000 30%, transparent 80%)`).
3. **noise:** feTurbulence fractalNoise com opacidade sutil de 0.05.
4. **vignette:** Gradiente radial elíptico obscurecendo as bordas externas.
- **Painel Central:** `rounded-2xl border border-white/10 bg-[rgba(32,32,38,0.40)] backdrop-blur-xl p-6 sm:p-8` com hairline zenital absoluta (`linear-gradient(90deg, transparent, rgba(131,80,242,0.6), transparent)`).

### Signature Components
- **Editor BBox com Atalhos (`gallery-bbox-editor`):** Toolbar com atalhos de teclado (B = Box, V = Mover/Selecionar, H = Pan, 1-4 = Seleção de Classes), zoom flutuante de 50% a 250% e canvas interativo de alta taxa de quadros.
- **Card de Telemetria do Nó (Sidebar):** Monitoramento contínuo em JetBrains Mono reportando consumo real de VRAM, CPU, RAM e contagem de jobs em execução.

### Iconografia Vetorial
Ícones com traço limpo, `strokeWidth="1.7"`, `viewBox="0 0 24 24"`, escala de 14px a 16px (`w-3.5 h-3.5` ou `w-4 h-4`), sem emojis funcionais:
- **Hardware & Sistema:** `Cpu`, `Server`, `Zap`, `Activity`
- **IA & Modelos:** `Target` (YOLO / Detecção), `Sparkles` (Difusão), `Layers` (OpenCLIP), `Wand` (AutoLabel)
- **Dados & Anotação:** `Database`, `Folder`, `Tag`, `BoxSelect`, `Crosshair`, `Grid`, `List`
- **Controle de Treino:** `Play`, `Pause`, `Stop`, `Refresh`
- **Ferramentas de Canvas:** `ZoomIn`, `ZoomOut`, `Eye`, `FileText`, `Sliders`
- **Navegação & Utilidades:** `Search`, `Download`, `Plus`, `Trash`, `X`, `ChevronDown`, `Check`, `MoreVertical`, `Terminal`

## Do's and Don'ts

### Do:
- **Do** manter exatamente um CTA outline-violeta translúcido (`border-brand-500/30 bg-brand-500/[0.12] text-white`) por contexto de ação.
- **Do** dar a todo botão o inset highlight zenital `rgba(255,255,255,0.08)` no topo (primário/destrutivo) ou `rgba(255,255,255,0.06)` (secundário).
- **Do** utilizar obrigatoriamente `JetBrains Mono` para todo dado quantitativo, telemetria, timestamp, path de arquivo e coordenadas de BBox.
- **Do** respeitar o anel de foco visível `outline: 2px solid #8350f2` com `outline-offset: 2px` em todos os elementos focáveis via teclado.
- **Do** suportar estritamente `prefers-reduced-motion: reduce`, desativando animações de pulso e transições dinâmicas.
- **Do** aplicar o vidro óptico de 3 níveis através das classes canônicas `.glass-card`, `.glass-menu` e `.glass-modal`.
- **Do** utilizar `truncate` acompanhado obrigatoriamente do atributo `title="..."` para exibição íntegra do valor.
- **Do** aplicar fade edge à direita de trilhos horizontais com overflow (pílulas de sub-navegação).

### Don't:
- **Don't** utilizar temas claros (Light Mode) sob hipótese alguma — o ambiente é exclusivamente Dark-Only.
- **Don't** utilizar emojis funcionais para ações, categorias ou status na interface — use apenas o set de ícones vetoriais com stroke 1.7.
- **Don't** aplicar sombras pretas opacas ou gradientes coloridos pesados em cards de fundo.
- **Don't** utilizar a cor sólida `bg-brand-500` como fundo de botão — o fundo de botão primário é translúcido `bg-brand-500/[0.12]` com borda outline `border-brand-500/30`.
- **Don't** utilizar classes `emerald-*` no código do app — elas ativam o verde legado da v1 no Tailwind v4.
- **Don't** aninhar `overflow-y-auto` pai e filho em colunas flexíveis (evitar armadilha de rolagem em viewport menor).
- **Don't** usar múltiplos botões com texto por extenso em toolbars móveis — colapsar para menu overflow "⋯".
- **Don't** permitir que títulos de tela ou da sidebar quebrem em duas linhas — truncar com badge inline.
