# task/impruvements/CorrigirImplementar.md

## Explicação

Arquivo responsavel por reunir todas as melhorias e correçoes de forma organizada e os items serão excluidos conforme forem concluido, é um arquivo fixo com conteudo temporario.

---

- Corrigir/Implementar:
  - Geração/Galeria (implementado/validar):
    - [ ] 002: A galeria não atualiza com uma nova foto, forçando o usuario atualizar. Caso de uso Usuario divide a tela em duas abas uma para a galeria e outra na geração ele tem que atualizar a pagina para ver a nova foto e bate no problema 001.
    - [ ] 006 Possibilidade de clicar na foto para copiar as configs(isso ainda não funciona 100%, não puxa o modelo e config do lora).

  - Infra:
- [x] Limitar o CI a apenas a branch main e develop as demais não devem ter CI.
- [ ] Plano de Autonomia e Independência de Infraestrutura (detalhado em `tasks/impruvements/infraestrutura-autonomia.md`):
  - [x] 01. .dockerignore completo (evitar contexts gigantes de build)
  - [x] 02. Fechar porta Postgres no host (`0.0.0.0:5432` -> `${DB_PUBLISH:-127.0.0.1}:5432`)
      - [x] 03. Passar `--network` no spawn de containers pelo Orchestrator
  - [x] 04. CI: rodar testes do workspace inteiro (`manager` e `orchestrator`) e engines Python
      - [ ] 05. Backup automatizado do Postgres (pgvector)
      - [ ] 06. Ingress/Reverse Proxy único para portas de produção
      - [ ] 07. Segregação de redes Docker Compose
      - [ ] 08. Containers com usuário não-privilegiado (`USER`)
      - [ ] 09. Métricas Prometheus e observabilidade dos serviços
      - [ ] 10. Limites de recursos (CPU/Memória) no compose

  - Backend, Orchestrator, Pipelines & Métricas:
    - [ ] Plano de Autonomia 24/7 (detalhado em `tasks/impruvements/backend-orchestrator-pipelines-autonomia.md`):
      - [x] 01. Correção de abort em `preparing` e loop do `job_prepares` (evitar zumbi/503)
      - [x] 02. Barreira local de GPU/VRAM e timeout com `kill_on_drop` no Orchestrator
      - [ ] 03. Deadlock de `SIGTERM` e dependências GPU (`torchao`) nas Engines
      - [ ] 04. Garbage Collection de sessões chunked, `outputs/`, `datasets-cache/` e S3
      - [x] 05. Retry e idempotência em `prepare_complete` e reports do orquestrador
      - [ ] 06. Instrumentação Prometheus (`/metrics`) nos 3 serviços Rust
      - [ ] 07. Tracing distribuído e propagação de `x-request-id` / `traceparent`
      - [ ] 08. Políticas de retenção de checkpoints locais por época
