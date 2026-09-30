# Biblioteca de Componentes UI — Hephaestus LLM Studio (Arcane UI v2.2)

> **Localização:** `apps/web/components/ui/`  
> **Importação:** `import { Button, Badge, GlassCard, ... } from "@/components/ui";`

Esta pasta reúne os componentes atômicos e primitivas do Design System, desenvolvidos com fidelidade estrita às regras do projeto definidas em `design-system.md` e `frontend.md`.

---

## Componentes Disponíveis

| Componente | Arquivo | Descrição |
|---|---|---|
| **[`Button`](./Button.tsx)** | `Button.tsx` | Botão atômico. Implementa "The One CTA Rule" (outline-violeta translúcido com inset highlight superior), além das variantes secundário glass, ghost, destrutivo e warning nos tamanhos `sm`, `md` e `lg`. Estado `loading`: desabilita, expõe `aria-busy="true"` e injeta texto `sr-only` "Carregando…" (anunciável); o `Spinner` interno é decorativo (`aria-hidden="true"`). |
| **[`GlassCard`](./GlassCard.tsx)** | `GlassCard.tsx` | Contêiner de vidro óptico em 3 níveis (`card`, `menu`, `modal`) com `border-top` reflexivo 1.8x a 2.5x mais luminoso. Inclui `GlassCardHeader`, `GlassCardTitle`, `GlassCardDescription`, `GlassCardBody` e `GlassCardFooter`. |
| **[`Badge`](./Badge.tsx)** | `Badge.tsx` | Cápsulas de status (`ready`, `alert`, `info`, `danger`), cápsula de telemetria, `brand` (studio-badge) e microtags mono. Suporte a ponto luminoso pulsante (`pulse`). |
| **[`Input`](./Input.tsx)** | `Input.tsx` | Campo de formulário estilizado em vidro escuro (`bg-black/40`), com suporte a `label` mono uppercase, ícones de prefixo/sufixo, mensagens de erro e proteção de fonte 16px no mobile. |
| **[`SearchInput`](./SearchInput.tsx)** | `SearchInput.tsx` | Input de busca especializado com ícone embutido, indicador de loading giratório (`spinner`) e botão de limpeza rápida. |
| **[`Select`](./Select.tsx)** | `Select.tsx` | Seletor customizado em dark glass com menu flutuante portaled (`useFloatingPosition`), navegação completa por teclado WAI-ARIA (`useListboxNavigation`), suporte a busca, agrupamento, tipografia mono/sans e tipagem genérica estrita `<T extends string | number>`. |
| **[`SegmentedControl`](./SegmentedControl.tsx)** | `SegmentedControl.tsx` | Alternador pill arredondado (`rounded-full`), conforme `Botão_estilo_grade_e_lista.png` para Grade vs Lista e grupos de opções mutuamente exclusivas. |
| **[`SubmodulePills`](./SubmodulePills.tsx)** | `SubmodulePills.tsx` | Trilho de abas e pílulas de filtro de categorias, com auto-scroll da pílula ativa, fade edge à direita e contadores em mono. |
| **[`Modal`](./Modal.tsx)** | `Modal.tsx` | Caixa de diálogo central Nível 3 (`.glass-modal`) com backdrop blur, linha zenital `hairline` em gradiente violeta no topo e fechamento por `Escape`. |
| **[`Drawer`](./Drawer.tsx)** | `Drawer.tsx` | Painel deslizante lateral retrátil (slide-over direita/esquerda) em vidro óptico com backdrop blur, listener de tecla `Escape`, trava de scroll do `body`, linha zenital reflexiva, cabeçalho e rodapé fixo. |
| **[`Slider`](./Slider.tsx)** | `Slider.tsx` | Controle deslizante tátil com thumb circular de 22px (#8350f2 com borda branca 2px), altura de trilho 8px e exibição do valor numérico em mono. |
| **[`ProgressBar`](./ProgressBar.tsx)** | `ProgressBar.tsx` | Barra de progresso com trilho `zinc-800` e preenchimentos semânticos (`#34d399` sucesso, `brand` violeta, âmbar e rosa). |
| **[`MetricTile`](./MetricTile.tsx)** | `MetricTile.tsx` | Bloco compacto de métricas quantitativas com rótulo em micro-caps e valor destacado em mono. |
| **[`StatCard`](./StatCard.tsx)** | `StatCard.tsx` | Card de KPI e métricas resumidas de alto nível em `.glass-card`, com ícone, rótulo em tracking mono, números tabulares grandes e subtexto. |
| **[`Breadcrumbs`](./Breadcrumbs.tsx)** | `Breadcrumbs.tsx` | Trilha de navegação com separadores em barra `/`, truncamento inteligente no segmento intermediário e realce no item ativo. |
| **[`TruncatedText`](./TruncatedText.tsx)** | `TruncatedText.tsx` | Implementa "The Truncamento Honesto Rule", garantindo que qualquer texto truncado (`truncate` ou `line-clamp`) exponha o atributo `title` com o conteúdo integral. |
| **[`EmptyState`](./EmptyState.tsx)** | `EmptyState.tsx` | Estado vazio amigável em vidro óptico com ícone, título, descrição e botão de ação. |
| **[`ConfirmDialog`](./ConfirmDialog.tsx)** | `ConfirmDialog.tsx` | Diálogo de confirmação de ações críticas com suporte a estado destrutivo (`danger`), busy loader e botão primário ou cancelamento. |
| **[`Toast`](./Toast.tsx)** | `Toast.tsx` | Sistema global de notificações e feedback óptico com auto-dismiss, ações interativas (`action`) e variante por tipo (`success`, `error`, `info`). |
| **[`DropOverlay`](./DropOverlay.tsx)** | `DropOverlay.tsx` | Overlay de drag & drop com borda tracejada violeta e feedback visual em backdrop-blur, acompanhado do hook `useFileDrop`. |
| **[`ZoomControl`](./ZoomControl.tsx)** | `ZoomControl.tsx` | Barra flutuante em `.glass-menu` para controle de escala e zoom de canvas interativos com indicador percentual mono e reset 100%. |
| **[`Kbd`](./Kbd.tsx)** | `Kbd.tsx` | Elemento atômico para indicação de atalhos de teclado (hotkeys) com estilo mono padronizado. |
| **[`Spinner`](./Spinner.tsx)** | `Spinner.tsx` | Indicador circular canônico de carregamento; puramente decorativo (`aria-hidden="true"` explícito — o anúncio fica com o pai). Tamanho livre via `className` (ex.: `size-3`…`size-8`), tom via `tone` (`brand`, `current`, `white`). Usado internamente por `Button` (loading) e `SearchInput`. |

---

## Design Tokens (`@theme` em `app/globals.css`)

**Cores semânticas de status** — use sempre a utility do token, nunca hex literal em `className`:

| Utility | Valor | Quando usar |
|---|---|---|
| `*-status-success` | `#34d399` | Sucesso, progresso concluído, classe 1 |
| `*-status-alert` | `#f59e0b` | Pausa, alerta de threshold, classe 2 |
| `*-status-danger` | `#ef4444` | Erro, aborto, exclusão, classe 3 |
| `*-status-telemetry` | `#06b6d4` | Latência/telemetria, selos AutoTracker, classe 4 |
| `*-status-runtime` | `#eab308` | Indicador de runtime Python/PyTorch |

Aceitam opacidade (`bg-status-alert/15`, `border-status-danger/30`). Tons claros de texto sobre véus (`amber-200/300/400`) são tinturas permitidas; o tom-base `amber-500` foi unificado a `status-alert`.

**Escala micro-tipográfica de dados** — substitui os antigos `text-[11px]/[10px]/[9px]` (cada token define `--text-*--line-height` explícita em `@theme`; `leading-*` nos call-sites sobrescreve o default):

| Utility | Tamanho / line-height | Uso |
|---|---|---|
| `text-2xs` | 11px / 1.4 | Captions compactas, logs, labels de toolbars |
| `text-3xs` | 10px / 1.4 | Micro-caps mono, `Kbd`, badges |
| `text-4xs` | 9px / 1.35 | Tags de canvas BBox, badges pico |

---

## Qualidade (Biome)

```bash
npm run lint --workspace=web    # cwd = raiz do monorepo; executa `biome lint .` com cwd apps/web (Checked 113 files; gate: 0 errors — atingido; 204 warnings + 3 infos reportados, não bloqueantes)
```

Config única em `biome.json` (raiz): `recommended` + `a11y`, `tailwindDirectives` ligado para o `@theme` do Tailwind v4. Formatter/assist habilitados mas não bloqueantes (storm de formatação vai em fatia própria — não rodar `--write` de `format`/`check` sem combinar).

---


## Exemplos Rápidos de Uso

```tsx
import { 
  Button, 
  Badge, 
  GlassCard, 
  GlassCardTitle, 
  Input, 
  ProgressBar,
  SegmentedControl 
} from "@/components/ui";
import { IconPlus, IconGrid, IconList } from "@/components/icons";

export function Example() {
  const [view, setView] = useState("grid");

  return (
    <GlassCard className="p-5">
      <div className="flex items-center justify-between">
        <GlassCardTitle>Meu Dataset</GlassCardTitle>
        <Badge variant="ready" pulse>Pronto</Badge>
      </div>

      <ProgressBar value={75} variant="success" label="Rotuladas" showPercent />

      <div className="flex items-center gap-2 mt-4">
        <SegmentedControl
          value={view}
          onChange={setView}
          options={[
            { id: "grid", icon: <IconGrid /> },
            { id: "list", icon: <IconList /> },
          ]}
        />
        <Button variant="primary" size="lg" leftIcon={<IconPlus />}>
          Novo Dataset
        </Button>
      </div>
    </GlassCard>
  );
}
```
