# Design System Specification — Hephaestus Studio

> **Referência Canônica:** [ai-vision-training-studio.html](file:///home/felipecn/DEV/tmp/ai-vision-training-studio.html)  
> **Identidade Visual:** OmniVision / Hephaestus AI Training & Annotation Suite  
> **Diretriz Estética:** Dark-Only de Alta Fidelidade, Glassmorphism Óptico, Densidade de Informação Técnica ("Anti-Slop")

---

## 1. Princípios de Design & Filosofia

1. **Foco Técnico e Densidade de Informação ("Anti-Slop"):**
   - A interface é projetada para engenheiros, pesquisadores e operadores de visão computacional.
   - Zero elementos puramente decorativos sem utilidade funcional. Cada pixel, métrica, curva ou tag transmite estado real do sistema.
   - Sem emojis para ações funcionais ou semânticas. Todos os controles usam ícones vetoriais monolínea com espessura uniforme (1.7px).

2. **Dark-Only Estrito:**
   - Paleta escura profunda com fundo `#090c12`, superfícies em cinza-zinco e acentos ópticos em esmeralda (`#10b981`).
   - Reduz a fadiga visual durante longas sessões de anotação de dados e monitoramento de treinos de modelos pesados.

3. **Economia de Ação (1 CTA Sólido por Workspace):**
   - Cada painel/workspace possui exatamente **um botão primário sólido** em destaque (`bg-emerald-500 text-zinc-950`), como "Iniciar Treinamento YOLO" ou "Salvar Dataset".
   - Ações secundárias e de controle usam botões de contorno, vidro sutil ou ghost buttons com bordas translúcidas.
   - Ações de pausa usam acento âmbar; abortar/excluir usam acento rosa/vermelho.

---

## 2. Design Tokens: Cores e Superfícies

### 2.1 Variáveis Nativas em OKLCH

O sistema utiliza o espaço de cores `oklch` para garantir transições tonais uniformes e alta saturação em monitores modernos:

```css
:root {
  --bg: oklch(13.5% 0.012 260);              /* #090c12 — Fundo base da aplicação */
  --surface: oklch(17.5% 0.016 260);         /* Superfícies de cards e painéis */
  --surface-elevated: oklch(21.5% 0.02 260); /* Superfícies elevadas (toolbars, headers) */
  --fg: oklch(96% 0.005 260);                /* #f1f3f9 — Texto principal de alto contraste */
  --muted: oklch(65% 0.015 260);             /* Texto secundário e labels */
  --border: oklch(26% 0.018 260);            /* Bordas estruturais */
  --accent: oklch(68% 0.20 150);             /* #10b981 — Acento primário Esmeralda */
  --accent-glow: oklch(68% 0.20 150 / 22%);  /* Halo de foco e pulso de treino */
}
```

### 2.2 Paleta Funcional & Semântica

| Papel Semântico | Cor / Token | Classes Tailwind | Uso na Interface |
|---|---|---|---|
| **Background Base** | `#090c12` | `bg-[#090c12]` | Fundo principal da página |
| **Acento Primário (Brand)** | `#10b981` | `emerald-500` / `brand-500` | CTA primário, status pronto, classe de sucesso, foco |
| **Sucesso / Treino Ativo** | `#34d399` | `emerald-400` | Barra de progresso, pulso de execução, mAP alto |
| **Alerta / Pausa** | `#f59e0b` | `amber-400` / `amber-500` | Botão pausar, classe 2 de BBox (`curto_circuito`) |
| **Perigo / Abortar / Perda** | `#ef4444` | `rose-400` / `rose-500` | Botão abortar, curva de Box Loss, classe 3 (`componente_ausente`) |
| **Telemetria / Auxiliar** | `#06b6d4` | `cyan-400` / `cyan-500` | Tag AutoTracker, latência Rust Core, classe 4 (`trilha_rompida`) |
| **Runtime / Python** | `#eab308` | `yellow-400` | Versão do motor Python / PyTorch |
| **Superfícies Zinc** | `zinc-950` / `zinc-900` | `bg-zinc-950`, `bg-zinc-900` | Headers, sidebars, cards, inputs |
| **Bordas Estruturais** | `zinc-800/80` | `border-zinc-800/80` | Divisórias e contornos |

---

## 3. Hierarquia de Vidro Óptico (Frosted Glassmorphism)

O design adota uma hierarquia de 3 níveis de profundidade de vidro com desfoque e saturação balanceados:

```
┌─────────────────────────────────────────────────────────────┐
│  Nível 3: .glass-modal (Modais e Diálogos de Sistema)       │
│  blur(32px) saturate(200%) · Sombra maciça 80px            │
├─────────────────────────────────────────────────────────────┤
│  Nível 2: .glass-menu (Dropdowns, Toast, Context Menus)     │
│  blur(28px) saturate(190%) · Borda topo com brilho 0.24    │
├─────────────────────────────────────────────────────────────┤
│  Nível 1: .glass-card (Cards de Datasets, Gráficos, Painéis)│
│  blur(20px) saturate(160%) · Fundo semi-translúcido 0.62   │
└─────────────────────────────────────────────────────────────┘
```

### Especificações CSS Exatas:

```css
/* Nível 1: Cards e Painéis */
.glass-card {
  background: rgba(18, 23, 35, 0.62);
  backdrop-filter: blur(20px) saturate(160%);
  -webkit-backdrop-filter: blur(20px) saturate(160%);
  border: 1px solid rgba(255, 255, 255, 0.07);
  border-top: 1px solid rgba(255, 255, 255, 0.13);
  box-shadow: 0 12px 30px -6px rgba(0, 0, 0, 0.45),
              inset 0 1px 0 rgba(255, 255, 255, 0.06);
}

.glass-card:hover {
  border-color: rgba(255, 255, 255, 0.16);
  background: rgba(22, 28, 42, 0.72);
}

/* Nível 2: Menus Flutuantes, Dropdowns e Toast */
.glass-menu {
  background: rgba(13, 17, 26, 0.82);
  backdrop-filter: blur(28px) saturate(190%);
  -webkit-backdrop-filter: blur(28px) saturate(190%);
  border: 1px solid rgba(255, 255, 255, 0.09);
  border-top: 1px solid rgba(255, 255, 255, 0.24);
  box-shadow: 0 24px 60px -8px rgba(0, 0, 0, 0.85), 
              0 0 0 1px rgba(255, 255, 255, 0.04),
              inset 0 1px 0 rgba(255, 255, 255, 0.18);
}

/* Nível 3: Modais Centrais */
.glass-modal {
  background: rgba(11, 15, 24, 0.88);
  backdrop-filter: blur(32px) saturate(200%);
  -webkit-backdrop-filter: blur(32px) saturate(200%);
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-top: 1px solid rgba(255, 255, 255, 0.28);
  box-shadow: 0 30px 80px -12px rgba(0, 0, 0, 0.9),
              0 0 0 1px rgba(255, 255, 255, 0.06),
              inset 0 1px 0 rgba(255, 255, 255, 0.2);
}
```

---

## 4. Tipografia e Microtipografia

### 4.1 Famílias Tipográficas
- **Sans (Interface & Textos):** `Inter`, `-apple-system`, `BlinkMacSystemFont`, `Segoe UI`, `Roboto`, `sans-serif`
- **Mono (Telemetria, Código, Coordenadas, Métricas):** `JetBrains Mono`, `IBM Plex Mono`, `ui-monospace`, `monospace`

### 4.2 Utilitários de Tracking
```css
/* Rótulos em caixa alta, badges, categorias e tags de cabeçalho */
.tracking-caps {
  letter-spacing: 0.08em;
}

/* Títulos, números grandes e displays de métricas */
.tracking-display {
  letter-spacing: -0.02em;
}
```

### 4.3 Escala Tipográfica
- **Micro (9px - 10px):** Categorias em uppercase (`tracking-caps`), badges de versão (`CUDA 12.4`, `Studio v1.3`), labels de classes.
- **Técnico (11px - 12px / `text-xs`):** Padrão do estúdio para labels de formulário, valores de tabelas, logs de terminal, coordenadas YOLO.
- **Leitura (13px - 14px / `text-sm`):** Títulos de cartões, botões de ação, itens de navegação de aba.
- **Destaque (16px - 18px / `text-base` a `text-lg`):** Valores de loss/mAP, títulos de workspaces, cabeçalho de modais.

---

## 5. Iconografia Vetorial de Precisão

Todos os ícones são construídos com linhas limpas, `strokeWidth="1.7"` e `viewBox="0 0 24 24"`, desenhados na escala de 14px a 16px (`w-3.5 h-3.5` ou `w-4 h-4`):

- **Hardware & Sistema:** `Cpu`, `Server`, `Zap`, `Activity`
- **IA & Modelos:** `Target` (YOLO / Detecção), `Sparkles` (Difusão), `Layers` (OpenCLIP / Camadas), `Wand` (AutoLabel / Invenção)
- **Dados & Anotação:** `Database`, `Folder`, `Tag`, `BoxSelect`, `Crosshair`, `Grid`, `List`
- **Controle de Treino:** `Play`, `Pause`, `Stop`, `Refresh`
- **Ferramentas de Canvas:** `ZoomIn`, `ZoomOut`, `Eye`, `FileText`, `Sliders`
- **Navegação & Utilidades:** `Search`, `Download`, `Plus`, `Trash`, `X`, `ChevronDown`, `Check`, `MoreVertical`, `Terminal`

---

## 6. Padrões Anatômicos de Componentes

### 6.1 Topbar do Studio (`studio-topbar`)
- Altura: `h-14` (56px), fixada no topo (`sticky top-0 z-40`), com `bg-zinc-950/80 backdrop-blur-xl`.
- **Lado Esquerdo:** Identificador de produto com badge de versão em cápsula, separador vertical e **seletor de ambiente** (`env-switcher-btn`) com indicador pulsante de status.
- **Lado Direito:** Telemetria com badges de latência Rust Core (`0.2ms`), versão do motor Python (`PyTorch 2.4.1`), barra visual de uso de VRAM com barra de progresso colorida por estado (Cyan quando ocioso, Esmeralda quando treinando) e botão de configurações.

### 6.2 Barra de Navegação Modular (`studio-tabs-bar`)
- Altura: `h-11` (44px), `bg-zinc-950 border-b border-zinc-800/80`.
- **Agrupamento Lógico com Separadores:**
  1. *Treino:* Difusão (`Flux·SDXL·1.5`), OpenCLIP (`Embedding`), YOLO (`v8/v9/v11`)
  2. *Preparo:* AutoLabel (`Difusão·CLIP`), AutoTracker (`Vídeo·Imagem`)
  3. *Dados:* Datasets (com contador dinâmico em badge mono)
- **Status Global à Direita:** Indicador de prontidão do daemon com bolinha animada (`animate-ping` durante execução).

### 6.3 Workspaces (Layout de Divisão 2 Colunas)
- **Coluna de Configuração (Esquerda):** Largura fixa de 320px a 384px (`w-full md:w-80 lg:w-96`), rolagem independente, contendo seletores com dropdown óptico, inputs numéricos em grid 2 colunas, sliders de taxa de aprendizado e o CTA primário de treino.
- **Coluna de Monitoramento/Visualização (Direita):** Fluida, com padding `p-4 md:p-6`, contendo banner de status ativo, cards de métricas em grade, gráficos SVG de convergência e terminal de logs com rolagem.

### 6.4 Editor de BBoxes da Galeria (`gallery-bbox-editor`)
- **Barra de Ferramentas:** Alternância entre Caixa (`B`), Mover/Selecionar (`V`) e Pan (`H`).
- **Paleta de Classes com Código de Atalho:**
  - `[1] solda_fria` → Verde Esmeralda (`bg-emerald-500`)
  - `[2] curto_circuito` → Âmbar (`bg-amber-500`)
  - `[3] componente_ausente` → Rosa/Vermelho (`bg-rose-500`)
  - `[4] trilha_rompida` → Ciano (`bg-cyan-500`)
- **Canvas com Zoom Flutuante:** Toolbar flutuante com zoom de 50% a 250% e botão de reset. Bounding boxes com coordenadas normalizadas (0 a 1), alças de redimensionamento nos cantos (`cursor-se-resize`), anel de foco `ring-2 ring-white/50` e etiqueta de identificação fixada no topo da caixa.

### 6.5 Feedback: Toasts Flutuantes & Menus de Contexto
- **Toast (`studio-toast`):** Fixado no canto inferior direito (`bottom-5 right-5`), classe `.glass-menu`, com indicador colorido por tipo (`emerald-400` para sucesso, `rose-400` para erro, `cyan-400` para info) e entrada suave via `@keyframes fade-in`.
- **Menu de Contexto (`glass-context-menu`):** Posicionamento em coordenadas absolutas do clique direito (`top/left`), cantos arredondados (`rounded-2xl`), divisores sutis em `white/10` e suporte a ações por entidade (dataset, imagem de amostra ou configurações).

---

## 7. Acessibilidade (WCAG 2.2 AA) e Movimento

1. **Foco Visível Acessível:**
   ```css
   button:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible, [role="tab"]:focus-visible {
     outline: 2px solid #10b981;
     outline-offset: 2px;
   }
   ```
2. **Contraste em Estados Desabilitados:**
   - Redução de opacidade para `0.55` e `cursor: not-allowed` é aplicada estritamente aos controles que possuem o atributo `disabled`.
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

---

## 8. Avaliação Crítica & Recomendações para a Base de Código

### Pontos Fortes do Design System Atual
- **Fidelidade Visual Superior:** O protótipo transmite a sensação de um software de nível industrial e especializado, similar a ferramentas de alta engenharia (como Linear, Raycast ou interfaces de observabilidade Datadog/Grafana Dark).
- **Consistência de Superfícies:** A regra de 3 camadas de vidro óptico confere profundidade sem recorrer a sombras opacas pesadas.
- **Ergonomia dos Controles:** Os atalhos visuais (B/V/H, 1-4 para classes) e o layout de 2 colunas maximizam a velocidade de trabalho em telas ultrawide ou laptops convencionais.

### Oportunidades de Otimização na Migração (Next.js / TS)
1. **Componentização Modular:** O protótipo reúne ~40 estados em um único componente `App()`. Na base real, extrair os átomos (`Button`, `Badge`, `Select`, `Modal`, `Toast`, `GlassCard`) e moléculas (`BBoxCanvas`, `MetricCard`, `LossChart`).
2. **Renderizador de Canvas Real:** No HTML, as caixas são representadas por `div`s absolutas com zoom via transform/dimensões. Na aplicação de produção, utilizar `<canvas>` nativo (ou biblioteca leve como Konva/Fabric) para suportar milhares de polígonos e anotações complexas sem gargalo de nós no DOM.
3. **Métricas Reativas:** Substituir os gráficos SVG estáticos por componentes reativos com suporte a séries temporais em streaming (via WebSocket) conforme as épocas e steps são concluídos pelos runners de GPU.
