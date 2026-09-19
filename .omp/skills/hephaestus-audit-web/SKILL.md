---
name: hephaestus-audit-web
description: Auditoria somente leitura de apps/web (Next.js 16 App Router, Tailwind v4, TypeScript strict, Biome, Impeccable) para padronizar interface, reduzir arquivos gigantes e código duplicado, e consolidar componentes do Studio. Produz tasks/web-modularizacao-auditoria.md.
---

# Objetivo
Fazer uma AUDITORIA SOMENTE LEITURA do `apps/web` (Hephaestus LLM Studio: Next.js 16 App Router, Tailwind v4, TypeScript strict, Biome) para padronizar a interface, reduzir arquivos gigantes e código duplicado, e consolidar um padrão único de componentes e de design.
"Independência e autonomia" significa: qualquer dev ou agente cria uma tela nova apenas compondo componentes existentes, sem copiar código de outra tela.

Documentação base: `docs/web/architecture.md`, `docs/web/state-and-realtime.md` — trate como HIPÓTESE, não como verdade. Registre toda divergência entre o documento e o código real.

# Regras
- NÃO edite código, NÃO reinicie serviços, NÃO instale dependências, NÃO altere package.json/lockfile.
- NÃO rode `next dev` nem `next build` (mexem no .next e podem interferir no dev server em execução).
- Comandos permitidos: wc, rg/grep, `tsc --noEmit`, `biome check` (SEM --write/--fix), e `npx jscpd|knip|madge` com saída em /tmp.
- O único arquivo criado é o entregável em `tasks/`.
- Escopo: `apps/web`. Leia `packages/contracts/openapi.yaml` só para comparar com os tipos.
- Toda afirmação precisa de evidência: caminho:linha e números (linhas, ocorrências, nº de cópias).

# Fase 0 — Contexto de design (Impeccable)
1. Leia a skill Impeccable instalada no projeto e siga o protocolo de coleta de contexto de design dela.
2. Se o contexto de design ainda não existir, NÃO rode o fluxo interativo de "teach" nem crie arquivos: registre isso como limitação e faça só a parte estrutural.
3. Use os princípios e anti-padrões da skill como régua visual, e as dimensões do comando audit (acessibilidade, performance, tema, responsividade, anti-padrões) como checklist.
4. Comandos que alteram código (normalize, extract, polish, harden etc.) NÃO devem ser executados agora. Use-os apenas como lente de análise: "o que o normalize ajustaria aqui?", "o que o extract tiraria daqui?".

# Fase 1 — Reconhecimento (agente principal)
1. Ler AGENTS.md, package.json, tsconfig, biome.json, next.config.ts, proxy.ts e o CSS global (tokens `@theme` do Tailwind v4).
2. Inventário quantitativo: top 30 arquivos .ts/.tsx por linhas, nº de arquivos por pasta, nº de arquivos com "use client".
3. Descobrir o que o architecture.md NÃO descreve: hooks, cliente de API/fetch, estado, polling de jobs, upload em blocos/MD5, utils, formulários.
4. Eleger 2–3 "referências de ouro" (os componentes ou telas que melhor seguem o padrão) como régua.
5. Propor o padrão-alvo, partindo do que já existe:
   - Tokens (`@theme`): cores, espaçamento, tipografia, z-index
   - `components/ui/`: primitivos sem regra de negócio (verificar se realmente não importam de `studio/` nem de `types/studio.ts`)
   - Compostos compartilhados: tabela, campo de formulário, diálogo de confirmação, estados loading/erro/vazio, uploader
   - `components/studio/`: separado por feature (treino, datasets, models, geração, anotação)
   - `app/(studio)/*/page.tsx`: só composição e carregamento de dados
   - Hooks/lib: dados, upload, polling e regras fora da UI
   - Types: por domínio, alinhados ao openapi

# Fase 2 — Varredura paralela (um subagente por fatia)
a) `components/ui`: cobertura (o que falta), API de props/variantes/tamanhos, foco, ARIA e teclado em Modal/Drawer
b) `components/studio`: arquivos grandes, responsabilidades misturadas, duplicação entre modais, painéis e galerias
c) Rotas, em 3 grupos: (datasets + [id] + annotate/[imageId]), (models + treino + geracao), (dashboard + jobs + environments + layout)
d) Dados/estado/API: fetch, tratamento de erro/401, polling, upload chunked, hooks
e) Estilos: hex/px/`[valor-arbitrário]` fora dos tokens, z-index, classNames longas repetidas (candidatas a variantes), estilos inline
f) `types/studio.ts` e utils: tamanho, divisão por domínio, desvio do openapi.yaml, se é gerado ou escrito à mão, formatadores duplicados (data, bytes, moeda)
g) Next.js: fronteira server/client ("use client" desnecessário, páginas inteiras client que poderiam ser server + ilhas), loading.tsx/error.tsx/not-found por rota
h) Lente Impeccable (Fase 0) sobre todas as rotas: acessibilidade, responsividade, tema, inconsistências visuais entre telas

Cada subagente devolve um relatório estruturado, sem implementar, procurando:
- Componentes > 300 linhas e hooks > 150 (top 20, com as responsabilidades misturadas)
- Fetch + estado + regra + UI no mesmo componente
- Markup repetido (modais, tabelas, cards, forms, headers) e lógica copiada
- Várias implementações do mesmo conceito (ex.: N modais, N formatadores de data)
- Estados de loading/erro/vazio tratados de formas diferentes
- Props inconsistentes, excesso de props, flags booleanas em vez de variantes
- Prop drilling, estado global desnecessário, dependências circulares, código morto
- Nomenclatura, estrutura de pastas e exports inconsistentes

# Fase 3 — Consolidação (agente principal)
- Deduplicar entre subagentes e validar por amostragem abrindo os arquivos citados
- Classificar cada item por: categoria, impacto (A/M/B), esforço (P/M/G), risco de regressão, prioridade (P0–P3), tipo (quick win / estrutural) e comando Impeccable indicado para executá-lo depois (normalize, extract, polish, harden etc.)
- Ordem sugerida: tokens e primitivos → compostos → quebrar arquivos gigantes → limpeza → polimento visual

# Entregável
Criar `tasks/web-modularizacao-auditoria.md`, seguindo as convenções de tasks do AGENTS.md, com:
1. Resumo executivo + métricas (top arquivos grandes, nº de duplicações, % de duplicação estimada, nº de "use client")
2. Divergências entre architecture.md e o código real
3. Padrão-alvo proposto (camadas, pastas, nomenclatura, regras de props, quando criar componente novo vs reutilizar)
4. Catálogo de componentes: existentes, duplicados e faltantes
5. Achados: tabela geral + cada tarefa com ID, evidência (caminho:linha), problema, proposta, critério de aceite, esforço, risco e dependências
6. Roadmap em fases (cada fase = um PR independente e verificável)
7. O que NÃO mudar e riscos
8. Texto sugerido de "Convenções de UI" para o AGENTS.md (só proposta; não edite o AGENTS.md)

No chat, responda só com os 5 achados mais críticos e o caminho do arquivo.
