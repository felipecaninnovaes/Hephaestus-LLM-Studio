# Design System: Hephaestus LLM Studio

> **Aviso de Consolidação da Documentação:**
> A documentação oficial e canônica do Design System foi padronizada e unificada em **[`docs/DESIGN.md`](./DESIGN.md)**, seguindo a especificação canônica do padrão DESIGN.md e do Impeccable (tokens de máquina no YAML frontmatter, sidecar `.impeccable/design.json`, 8 seções normativas de estilo e regras nomeadas).
>
> **Consulte sempre [`docs/DESIGN.md`](./DESIGN.md) como a única fonte de verdade de ESTILO para implementadores e designers.**

---

## Sumário Rápido de Referência (Arcane v2/v2.1)

- **North Star:** "The Arcane Foundry" (dark-only de precisão, laboratório industrial de IA).
- **Paleta Primária:** Fundo `#0d0d0d`, acento Violeta Arcane `#8350f2` (`brand-500`), neutros `zinc-*` com undertone berinjela.
- **Regra Brand-Only:** Classes `emerald-*` são **estritamente proibidas** no código (resolvem para a v1 verde no Tailwind v4). Cores semânticas de sucesso usam `#34d399`.
- **Regra One CTA:** Exatamente um CTA outline-violeta translúcido (`border-brand-500/30 bg-brand-500/[0.12] text-white`) por painel. Botão sólido `bg-brand-500` é **proibido**.
- **Tipografia:** `Space Grotesk` (títulos), system sans (corpo/controles), `JetBrains Mono` (números, telemetria, logs, BBoxes). Fontes self-hosted em `apps/web/fonts/*.woff2`.
- **Densidade:** `html { font-size: 14px }` (densidade compacta Arcane v2.1; hit-area mínima 28px).
- **Elevação:** Vidro óptico em 3 níveis (`.glass-card`, `.glass-menu`, `.glass-modal`) com topo iluminado zenital (`border-top`).
- **Navegação & Layout:** Sidebar macro à esquerda (drawer retrátil `min(85vw, 320px)` em telas móveis), breadcrumbs em 1 linha, pílulas com fade edge e regra Anti-Scroll-Trap.
- **Login:** Painel central translúcido sobre fundo `AuthAmbient` (mesh violeta, grade 48px com máscara radial, noise 5% e vignette).

Para detalhes completos de tokens, componentes, regras de acessibilidade e Do's and Don'ts, acesse **[`docs/DESIGN.md`](./DESIGN.md)**.
