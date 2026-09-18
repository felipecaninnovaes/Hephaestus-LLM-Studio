# Estado e Tempo Real (`apps/web`)

Este documento descreve os mecanismos de sincronização de estado, transmissão de eventos em tempo real e comunicação entre abas no frontend do Hephaestus LLM Studio. A arquitetura prioriza resiliência, utilizando Server-Sent Events (SSE) para telemetria de jobs e canais nativos do navegador para sincronização local e cross-tab.

---

## Telemetria de Jobs em Tempo Real

A execução de tarefas de longa duração (treinamento YOLO, treino de LoRA, geração por difusão, autolabel) emite eventos estruturados consumidos reativamente pela interface:

### 1. Hook `useJobTelemetry` (`hooks/useJobTelemetry.ts`)
- **Conexão SSE Primária**: Conecta ao endpoint unificado `/api/jobs/:id/events` do BFF (`api-principal`).
- **Estados Reativos**:
  - `phase` e `phaseMessage`: Fase atual do pipeline (ex.: inicialização, carregamento de pesos, amostragem, salvamento).
  - `progress`: Progresso linear normalizado entre `0.0` e `1.0`.
  - `step`/`totalSteps` e `epoch`/`totalEpochs`: Contadores de iterações do treinamento.
  - `vramUsedGb`: Consumo de memória de GPU capturado em tempo real das engines.
  - `metrics`: Dicionário dinâmico de perdas (*losses*) e métricas de acurácia (*mAP*).
- **Mecanismo de Fallback**: Caso a conexão SSE seja abortada por proxies intermediários ou falha transitória de rede, o hook ativa automaticamente polling periódico via `getJob(jobId)` até o restabelecimento do stream ou finalização do job.

### 2. Visualizador de Execução (`JobLogViewer.tsx`)
- Renderiza o fluxo de logs brutos e estruturados do job em tempo real.
- Classifica linhas por tags de subsistema (`ORCH`, `ENGINE`, `TRAIN`, `DIFFUSION`, `AUTOLABEL`, `S3`, `STDERR`, `WARN`).
- Suporta rolagem automática inteligente com retenção de posição caso o operador role para cima, além de cópia rápida para a área de transferência.

---

## Central de Ações (`ActionCenter.tsx`)

O `ActionCenter` é a gaveta lateral (*Drawer*) global do estúdio:
- **Ativação Desacoplada**: Aberto globalmente de qualquer página por atalhos ou disparo do evento customizado `hephaestus:open-action-center`.
- **Monitoramento Ativo**: Acompanha a fila de jobs e jobs em execução em segundo plano sem exigir que o usuário permaneça na página `/jobs`.
- **Pontos de Decisão**: Dispara diálogos de revisão de dados gerados por autolabel (`AutolabelReviewModal`) e autotrack (`AutotrackerReviewModal`) antes de efetivar modificações no banco de dados.

---

## Galeria de Geração e Sincronização Cross-Tab

A tela de difusão (`/geracao`) coordena o painel de parâmetros (`GenerationPanel`) e a galeria de resultados (`GenerationGallery`), suportando uso simultâneo em múltiplas abas:

### 1. Persistência de Formulário (`lib/geracao-storage.ts`)
- Configurações do formulário (modelo, LoRAs, prompts, resolução, passos, sampler, sementes) são persistidas em `localStorage` sob a chave versionada `geracao:form:v1`.
- Não armazena imagens binárias ou segredos no storage local, mantendo o consumo de cota enxuto e seguro.

### 2. Notificação e Atualização Cross-Tab
Quando uma nova geração de imagem é concluída:
- **`BroadcastChannel`**: Publica o evento `generation_completed` no canal `hephaestus:geracao`.
- **Evento `storage`**: Como garantia retrocompatível, atualiza a chave `geracao:lastCompletedAt`. Abas secundárias abertas na galeria escutam a alteração e atualizam a listagem sem intervenção manual.
- **Mesma Aba**: A galeria sincroniza ao alternar abas de foco via ouvintes nos eventos de janela `visibilitychange` e `focus`.

### 3. Reuso de Parâmetros e Init Image
- Ao clicar em "Reutilizar Configuração" ou "Usar como Imagem Inicial", a galeria publica os parâmetros na chave `geracao:initSource` e emite o evento `hephaestus:apply-geracao-form`, re-hidratando o formulário instantaneamente.
