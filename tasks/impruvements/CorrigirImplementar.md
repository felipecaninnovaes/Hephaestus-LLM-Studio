# task/impruvements/CorrigirImplementar.md

## Explicação

Arquivo responsavel por reunir todas as melhorias e correçoes de forma organizada e os items serão excluidos conforme forem concluido, é um arquivo fixo com conteudo temporario.

---

- Corrigir/Implementar:
  - Geração/Galeria (implementado/validar):
    - [x] 002: A galeria não atualiza com uma nova foto, forçando o usuario atualizar. Caso de uso Usuario divide a tela em duas abas uma para a galeria e outra na geração ele tem que atualizar a pagina para ver a nova foto e bate no problema 001.
    - [x] 006 Possibilidade de clicar na foto para copiar as configs(isso ainda não funciona 100%, não puxa o modelo e config do lora).

  - Infra:
- [x] Limitar o CI a apenas a branch main e develop as demais não devem ter CI.
- [ ] Plano de Autonomia e Independência de Infraestrutura (detalhado em `tasks/impruvements/infraestrutura-autonomia.md`):
  - [x] 01. .dockerignore completo (evitar contexts gigantes de build)
  - [x] 02. Fechar porta Postgres no host (`0.0.0.0:5432` -> `${DB_PUBLISH:-127.0.0.1}:5432`)
      - [x] 03. Passar `--network` no spawn de containers pelo Orchestrator
  - [x] 04. CI: rodar testes do workspace inteiro (`manager` e `orchestrator`) e engines Python
      - [x] 05. Backup automatizado do Postgres (pgvector)
      - [x] 06. Ingress/Reverse Proxy único para portas de produção
      - [x] 07. Segregação de redes Docker Compose
      - [x] 08. Containers com usuário não-privilegiado (`USER`)
      - [x] 09. Métricas Prometheus e observabilidade dos serviços
      - [x] 10. Limites de recursos (CPU/Memória) no compose
  - Backend, Orchestrator, Pipelines & Métricas:
    - [ ] Plano de Autonomia 24/7 (detalhado em `tasks/impruvements/backend-orchestrator-pipelines-autonomia.md`):
      - [x] 01. Correção de abort em `preparing` e loop do `job_prepares` (evitar zumbi/503)
      - [x] 02. Barreira local de GPU/VRAM e timeout com `kill_on_drop` no Orchestrator
      - [x] 03. Deadlock de `SIGTERM` e dependências GPU (`torchao`) nas Engines
      - [x] 04. Garbage Collection de sessões chunked, `outputs/`, `datasets-cache/` e S3
      - [x] 05. Retry e idempotência em `prepare_complete` e reports do orquestrador
      - [x] 06. Instrumentação Prometheus (`/metrics`) nos 3 serviços Rust
      - [x] 07. Tracing distribuído e propagação de `x-request-id` / `traceparent`
      - [x] 08. Políticas de retenção de checkpoints locais por época
  - Modularização das Engines & Eliminação de Duplicações:
    - [ ] Plano de Modularização das Engines (detalhado em `tasks/impruvements/modularizacao-engines-autonomia.md`):
      - [x] 01. Extração do pacote compartilhado stdlib-only `engine-kit` (telemetry, mock, runtime, vram, httpd, artifacts)
      - [x] 02. Adoção do `engine-kit` nas 3 engines e correção de drift (VRAM 1023³, parsing tolerante de `ENGINE_MOCK`)
      - [x] 03. Decomposição de `generate.py` (1.709L) no pacote `generation/` com facades de retrocompatibilidade
      - [x] 04. Decomposição de `models/flux.py` (1.586L) no pacote `models/flux/` (rope, encoding, quant, loop)
      - [x] 05. Decomposição de `common.py` (760L) em `common_pkg/` e unificação de loops de treino SD
      - [x] 06. Modularização de `trainer-yolo` (`yolo_adapter.py`, `autolabel/`, `config.py`, `deterministic.py`)
      - [x] 07. Modularização de `trainer-clip/serve.py` e suite de testes de contrato
