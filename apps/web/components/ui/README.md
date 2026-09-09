# Biblioteca de Componentes UI — Hephaestus LLM Studio (Arcane UI v2.1)

> **Localização:** `apps/web/components/ui/`  
> **Importação:** `import { Button, Badge, GlassCard, ... } from "@/components/ui";`

Esta pasta reúne os componentes atômicos e primitivas do Design System, desenvolvidos com fidelidade estrita às regras do projeto definidas em `design-system.md` e `frontend.md`.

---

## Componentes Disponíveis

| Componente | Arquivo | Descrição |
|---|---|---|
| **[`Button`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Button.tsx)** | `Button.tsx` | Botão atômico. Implementa "The One CTA Rule" (outline-violeta translúcido com inset highlight superior), além das variantes secundário glass, ghost, destrutivo e warning nos tamanhos `sm`, `md` e `lg`. |
| **[`GlassCard`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/GlassCard.tsx)** | `GlassCard.tsx` | Contêiner de vidro óptico em 3 níveis (`card`, `menu`, `modal`) com `border-top` reflexivo 1.8x a 2.5x mais luminoso. Inclui `GlassCardHeader`, `GlassCardTitle`, `GlassCardDescription`, `GlassCardBody` e `GlassCardFooter`. |
| **[`Badge`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Badge.tsx)** | `Badge.tsx` | Cápsulas de status (`ready`, `alert`, `info`, `danger`), cápsula de telemetria, `brand` (studio-badge) e microtags mono. Suporte a ponto luminoso pulsante (`pulse`). |
| **[`Input`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Input.tsx)** | `Input.tsx` | Campo de formulário estilizado em vidro escuro (`bg-black/40`), com suporte a `label` mono uppercase, ícones de prefixo/sufixo, mensagens de erro e proteção de fonte 16px no mobile. |
| **[`SearchInput`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/SearchInput.tsx)** | `SearchInput.tsx` | Input de busca especializado com ícone embutido, indicador de loading giratório (`spinner`) e botão de limpeza rápida. |
| **[`Select`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Select.tsx)** | `Select.tsx` | Seletor nativo estilizado em dark glass, chevron customizado e suporte a tipografia mono ou sans. |
| **[`SegmentedControl`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/SegmentedControl.tsx)** | `SegmentedControl.tsx` | Alternador pill arredondado (`rounded-full`), conforme `Botão_estilo_grade_e_lista.png` para Grade vs Lista e grupos de opções mutuamente exclusivas. |
| **[`SubmodulePills`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/SubmodulePills.tsx)** | `SubmodulePills.tsx` | Trilho de abas e pílulas de filtro de categorias, com auto-scroll da pílula ativa, fade edge à direita e contadores em mono. |
| **[`Modal`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Modal.tsx)** | `Modal.tsx` | Caixa de diálogo central Nível 3 (`.glass-modal`) com backdrop blur, linha zenital `hairline` em gradiente violeta no topo e fechamento por `Escape`. |
| **[`Slider`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Slider.tsx)** | `Slider.tsx` | Controle deslizante tátil com thumb circular de 22px (#8350f2 com borda branca 2px), altura de trilho 8px e exibição do valor numérico em mono. |
| **[`ProgressBar`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/ProgressBar.tsx)** | `ProgressBar.tsx` | Barra de progresso com trilho `zinc-800` e preenchimentos semânticos (`#34d399` sucesso, `brand` violeta, âmbar e rosa). |
| **[`MetricTile`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/MetricTile.tsx)** | `MetricTile.tsx` | Bloco compacto de métricas quantitativas com rótulo em micro-caps e valor destacado em mono. |
| **[`Breadcrumbs`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/Breadcrumbs.tsx)** | `Breadcrumbs.tsx` | Trilha de navegação com separadores em barra `/`, truncamento inteligente no segmento intermediário e realce no item ativo. |
| **[`TruncatedText`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/TruncatedText.tsx)** | `TruncatedText.tsx` | Implementa "The Truncamento Honesto Rule", garantindo que qualquer texto truncado (`truncate` ou `line-clamp`) exponha o atributo `title` com o conteúdo integral. |
| **[`EmptyState`](file:///home/felipecn/DEV/redesign/apps/web/components/ui/EmptyState.tsx)** | `EmptyState.tsx` | Estado vazio amigável em vidro óptico com ícone, título, descrição e botão de ação. |
| **[`ConfirmDialog`](file:///home/felipecn/DEV/Hephaestus-LLM-Studio/apps/web/components/ui/ConfirmDialog.tsx)** | `ConfirmDialog.tsx` | Diálogo de confirmação de ações críticas com suporte a estado destrutivo (`danger`), busy loader e botão primário ou cancelamento. |
| **[`Toast`](file:///home/felipecn/DEV/Hephaestus-LLM-Studio/apps/web/components/ui/Toast.tsx)** | `Toast.tsx` | Sistema global de notificações e feedback óptico com auto-dismiss, ações interativas (`action`) e variante por tipo (`success`, `error`, `info`). |
| **[`DropOverlay`](file:///home/felipecn/DEV/Hephaestus-LLM-Studio/apps/web/components/ui/DropOverlay.tsx)** | `DropOverlay.tsx` | Overlay de drag & drop com borda tracejada violeta e feedback visual em backdrop-blur, acompanhado do hook `useFileDrop`. |
| **[`ZoomControl`](file:///home/felipecn/DEV/Hephaestus-LLM-Studio/apps/web/components/ui/ZoomControl.tsx)** | `ZoomControl.tsx` | Barra flutuante em `.glass-menu` para controle de escala e zoom de canvas interativos com indicador percentual mono e reset 100%. |

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
