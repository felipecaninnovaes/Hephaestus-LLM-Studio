# task/impruvements/CorrigirImplementar.md

## Explicação

Arquivo responsavel por reunir todas as melhorias e correçoes de forma organizada e os items serão excluidos conforme forem concluido, é um arquivo fixo com conteudo temporario.

---

- Corrigir/Implementar:
  - Geração/Galeria (implementado/validar):
    - [ ] 002: A galeria não atualiza com uma nova foto, forçando o usuario atualizar. Caso de uso Usuario divide a tela em duas abas uma para a galeria e outra na geração ele tem que atualizar a pagina para ver a nova foto e bate no problema 001.
    - [ ] 006 Possibilidade de clicar na foto para copiar as configs(isso ainda não funciona 100%, não puxa o modelo e config do lora).

  - Infra:
    - [ ] Limitar o CI a apenas a branch main e develop as demais não devem ter CI.
    - [ ] Plano de Autonomia e Independência de Infraestrutura (detalhado em `tasks/impruvements/infraestrutura-autonomia.md`):
      - [ ] 01. .dockerignore completo (evitar contexts gigantes de build)
      - [ ] 02. Fechar porta Postgres no host (`0.0.0.0:5432`)
      - [ ] 03. Passar `--network` no spawn de containers pelo Orchestrator
      - [ ] 04. CI: rodar testes do workspace inteiro (`manager` e `orchestrator`) e engines Python
      - [ ] 05. Backup automatizado do Postgres (pgvector)
      - [ ] 06. Ingress/Reverse Proxy único para portas de produção
      - [ ] 07. Segregação de redes Docker Compose
      - [ ] 08. Containers com usuário não-privilegiado (`USER`)
      - [ ] 09. Métricas Prometheus e observabilidade dos serviços
      - [ ] 10. Limites de recursos (CPU/Memória) no compose
