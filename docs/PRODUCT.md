# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Desenvolvedores e pesquisadores de IA/ML, engenheiros de visão computacional e praticantes técnicos que precisam de um fluxo unificado para:
- Curar, estruturar e gerenciar datasets de visão e difusão;
- Executar anotação assistida e acelerada por modelos de IA (legendas/captions e bounding boxes);
- Configurar e orquestrar pipelines de treinamento com alternância flexível entre hardware local (GPU própria) e instâncias remotas na nuvem (VPS ou RunPod).

## Product Purpose

Prover um estúdio visual unificado, modular e auto-hospedado (local-first com suporte a nuvem) que centraliza o ciclo completo de preparação de dados e treinamento de modelos de IA de visão e difusão. O produto resolve a extrema fragmentação do ecossistema atual — onde desenvolvedores precisam alternar entre ferramentas desconexas (como interfaces avulsas de anotação, scripts manuais de conversão de formato e webUIs isoladas) —, proporcionando governança centralizada de datasets, telemetria de hardware em tempo real e orquestração transparente de jobs.

## Positioning

Estúdio de engenharia de IA de ponta a ponta que integra ferramentas especializadas de dados (AutoLabel e AutoTracker) diretamente ao ciclo de treino (Rust Core + Orquestrador Rust + PyTorch), com armazenamento canônico em Object Storage S3 compatível (SeaweedFS). Diferencia-se de plataformas proprietárias e SaaS por oferecer controle soberano dos dados e modelos, sem vendor lock-in e com paridade operacional idêntica entre GPU local e instâncias de alto desempenho na nuvem.

## Operating Context

- **Ambientes de Execução:** Comutação contínua via interface entre Docker Local (GPU do usuário) e infraestrutura remota (Pods RunPod, instâncias VPS com GPUs dedicadas como A100/L40S).
- **Telemetria de Infraestrutura:** Acompanhamento contínuo de latência da API Rust Core, versão do motor Python/PyTorch, consumo de VRAM da GPU (em repouso vs. em treino) e streaming de logs de execução via WebSocket.
- **Fluxos de Dados e Mídia:** Upload resiliente via API central em blocos; imagens canônicas em bucket S3/SeaweedFS; anotações e metadados no banco relacional (PostgreSQL) e no wire; exportação/importação estruturada de pacotes de dataset (ZIP contendo imagens, anotações, `dataset.yaml` e labels).

## Capabilities and Constraints

- **Módulos de Treinamento Especializados:**
  - *Difusão:* Fine-tuning e LoRA para modelos gerativos de imagem (Flux, SDXL, Stable Diffusion 1.5);
  - *Embeddings e Descrição:* Treinamento de representações visuais com OpenCLIP;
  - *Detecção e Rastreamento:* Treinamento de visão computacional com YOLO (v8, v9, v11).
- **Ferramentas Integradas de Preparo de Dados:**
  - *AutoLabel:* Geração automática de descrições e legendas para difusão e CLIP, suportando modelos locais, APIs externas no padrão OpenAI e upload de modelos customizados;
  - *AutoTracker:* Detecção e rastreamento automático com geração de bounding boxes em imagens e vídeos para YOLO, com editor visual em canvas para refinamento manual e suporte a upload de modelos customizados.
- **Gestão Centrada em Datasets:**
  - Exibição de datasets em grade ou lista;
  - Acesso à galeria completa mediante clique no dataset (AutoLabel, AutoTracker, importação e exportação de backups concentram-se no contexto da galeria);
  - Status derivado e consistente do ciclo de vida dos dados (`needs_labeling`, `in_progress`, `ready`).
- **Restrições Arquiteturais e de Wire:**
  - Convenção de casing rígida: camelCase em toda a API REST pública (`/api/*`), snake_case no banco de dados e arquivos de manifesto;
  - Backend Rust central atua como único ponto de contato da interface web, controlando autenticação por sessão/cookie, autorização e acesso ao storage.

## Brand Commitments

- **Nome Oficial:** Hephaestus LLM Studio.
- **Identidade e Design System:** Tema exclusivo dark ("The Arcane Foundry": fundo base `#0d0d0d`, superfícies translúcidas em vidro óptico com undertone berinjela, acento brand violeta `#8350f2` com destaques semânticos para métricas e classes). Superfícies com glassmorphism em 3 níveis (.glass-menu, .glass-card, .glass-modal). Tipografia em Space Grotesk para display/títulos, system sans para UI e JetBrains Mono para telemetria, dados e código.
- **Voz e Tom:** Técnico, direto, utilitário e confiável, voltado para engenharia de alto desempenho.

## Evidence on Hand

- Referência visual: `docs/DESIGN.md` (Design System unificado Arcane v2/v2.1) realizando-se em `apps/web/`; protótipos antigos aposentados e removidos do repo.
- Especificações e decisões arquiteturais documentadas: `IDEIA.md`, `docs/frontend.md`, `docs/backend.md`, `docs/repo-estrutura.md` e ADRs estruturadas (`docs/adr/0001`, `docs/adr/0002`, `docs/adr/0003`).
- Base de código web em desenvolvimento ativo em `apps/web` (Next.js 16, React 19, Tailwind CSS v4, design tokens integrados em `globals.css`).

## Product Principles

1. **Soberania e Autonomia Local:** O estúdio pertence ao usuário; dados, modelos, anotações e pesos de treino permanecem sob controle total em infraestrutura própria, operando perfeitamente sem dependências proprietárias externas.
2. **Eliminação de Fragmentação:** O ciclo completo — ingestão, anotação assistida, revisão manual e treino — vive em uma experiência coesa e fluida, dispensando ferramentas externas desconexas ou scripts intermediários.
3. **Hardware e Telemetria Transparentes:** Recursos de hardware (VRAM, latência, pods remotos) são monitorados com precisão e expostos de forma clara para decisões conscientes de capacidade e custo.
4. **Governança Centrada no Dataset:** Datasets são os artefatos fundamentais do sistema; a galeria de cada dataset é a fonte única de verdade para curadoria, anotação e backup reprodutível.

## Accessibility & Inclusion

- Conformidade com as diretrizes WCAG 2.2 AA.
- Foco visível de alto contraste (`outline: 2px solid #8350f2`), estados desabilitados claros com contraste mantido e navegação integral por teclado em fluxos modais e editores de anotação.
- Respeito irrestrito a `prefers-reduced-motion: reduce` para desativar efeitos visuais de scan e pulso.
