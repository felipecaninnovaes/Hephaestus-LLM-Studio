---
name: hephaestus-audit-backend
description: Auditoria somente leitura do backend em Rust (services/api-principal :8080, services/manager :8081, services/orchestrator :8082) para padronizar arquitetura interna, reduzir monólitos, eliminar duplicação entre serviços e blindar contratos. Produz tasks/backend-modularizacao-auditoria.md.
---

# Objetivo
Fazer uma AUDITORIA SOMENTE LEITURA do backend em Rust do Hephaestus LLM Studio — `services/api-principal` (:8080, BFF), `services/manager` (:8081, fila/estado) e `services/orchestrator` (:8082, agente de nó GPU) — para padronizar a arquitetura interna, reduzir arquivos/funções gigantes, eliminar duplicação (inclusive ENTRE os serviços) e deixar cada serviço fácil de manter e evoluir.
"Independência e autonomia" significa: cada serviço pode evoluir e ser testado sem tocar nos outros, e o contrato (`packages/contracts/openapi.yaml`) é a única fonte de verdade entre serviços e web.

Documentação base: `docs/services/api-principal.md`, `docs/services/manager.md`, `docs/services/orchestrator.md` — trate como HIPÓTESE. Registre toda divergência entre a documentação e o código real.

# Regras
- NÃO edite código, NÃO reinicie serviços, NÃO instale nada (nem `cargo install`), NÃO altere Cargo.toml/Cargo.lock, NÃO rode `cargo fmt`/`cargo fix`/`cargo add`/`cargo update`.
- NÃO mexa em containers, no Docker socket, no banco, no S3 nem em nós GPU. NÃO rode testes que dependam deles.
- NÃO leia nem imprima `.env`, segredos, tokens ou chaves. Se encontrar segredo em texto claro no repositório, cite só o caminho:linha, sem o valor.
- Para compilar/analisar, SEMPRE use `CARGO_TARGET_DIR=/tmp/heph-audit-target` e `--locked`, para não tocar em `target/` nem disparar hot-reload (cargo-watch etc.).
- Comandos permitidos: wc/tokei, rg, `cargo check`, `cargo clippy`, `cargo fmt --check`, `cargo tree -d`, `cargo test --no-run` (só compila), e ferramentas JÁ instaladas (cargo-machete, cargo-audit, cargo-deny, jscpd via npx com saída em /tmp). Se uma ferramenta faltar, registre como recomendação.
- Toda afirmação precisa de evidência: caminho:linha e números.
- Só o entregável em `tasks/` pode ser criado.

# Invariantes a preservar (não propor mudar sem justificativa forte)
Os documentos descrevem estas decisões como deliberadas; a auditoria deve verificar se o código realmente as respeita e proteger cada uma:
- Upload chunked com streaming direto para disco (RAM O(1)), partes de até 96 MiB, TempDir com limpeza no drop e sweep de sessões abandonadas
- SSE de telemetria que só emite quando há mudança real e encerra em estado terminal
- Máquina de estados de jobs: queued → preparing → running → done/failed/cancelled
- Watchdogs do manager (heartbeat stale > 10s, prepare_timeout de 60 min, reconciliação de órfãos) com tempos dimensionados para transferências pesadas
- Orchestrator stateless, sweep de containers e workdirs no boot, `kill_on_drop(true)` em todo subprocesso, TTL do daemon de difusão
- Manager não exposto publicamente, acessado só por api-principal e nós via MANAGER_TOKEN

# Fase 1 — Reconhecimento (agente principal)
1. Ler AGENTS.md, o Cargo.toml do workspace e de cada serviço (edition, rust-version, deps, features), clippy.toml/rustfmt.toml/deny.toml (se existirem), migrations SQL, `packages/contracts/openapi.yaml` e `packages/policies/vram-table.yaml`.
2. Inventário quantitativo por serviço: linhas por arquivo (top 30 .rs), nº de módulos, e contagens de `unwrap()/expect()/panic!` fora de testes, `unsafe`, `#[allow(...)]`, `TODO/FIXME`, `tokio::spawn`.
3. Descobrir o que a documentação NÃO diz: layout de módulos/crates, existência de crates compartilhados, camada de acesso a dados (sqlx? qual?), cliente S3, tratamento de erros (anyhow/thiserror), config/env, logging (tracing), testes.
4. Eleger 2–3 "referências de ouro" (os módulos que melhor seguem um bom padrão) como régua.
5. Propor a arquitetura-alvo, partindo do que já existe:
   - Transporte (routes/handlers): extrai, valida, chama serviço, mapeia resposta. Sem SQL e sem regra de negócio
   - Aplicação (services/use cases)
   - Domínio: tipos, máquina de estados de jobs, políticas de VRAM. Puro e testável
   - Infra: repositórios (SQL), S3, Docker/subprocessos, clientes HTTP internos, sempre atrás de traits quando isso ajudar testes
   - Bootstrap: `main.rs` mínimo (config tipada, tracing, wiring, graceful shutdown)
   - Crates compartilhados candidatos: DTOs/contrato e protocolo interno (dispatch/report/heartbeat), modelo de erro, config/env, storage S3, telemetria/tracing, auth interna (MANAGER_TOKEN)

# Fase 2 — Varredura paralela (um subagente por fatia)
a) api-principal — auth, validação de entrada (imagens, MD5, sanitização), datasets, search, generations
b) api-principal — models + upload chunked, jobs, SSE de telemetria
c) manager — scheduler/dispatch, máquina de estados, watchdogs, registro de nós/heartbeats, políticas de VRAM
d) orchestrator — handlers de dispatch/report, runner de containers Docker, sweeps, daemon de difusão, cache/workdir
e) Transversal, contratos e duplicação entre serviços: DTOs duplicados, protocolo interno (dispatch/report/heartbeat) escrito de forma independente em cada lado, drift entre openapi.yaml e handlers/DTOs (gerado ou manual?), modelo de erro, config/env, tracing, cliente S3, clientes HTTP
f) Banco de dados: onde ficam as queries, migrations, transações, N+1, e qual serviço é dono de quais tabelas (api-principal e manager acessam o mesmo Postgres?)
g) Robustez async: bloqueio dentro de async (std::fs, std::process), Mutex std segurado através de `.await`, `tokio::spawn` sem supervisão, canais sem limite, timeouts ausentes em chamadas externas (reqwest, Docker, S3), cancelamento, graceful shutdown, retry/backoff, TODO `Command` sem `kill_on_drop`, estado só em memória (ex.: sessões de upload)
h) Testes e tooling: cobertura por módulo, testabilidade (dependências concretas vs traits), helpers de teste duplicados, deps duplicadas/não usadas, lints, tempo de compilação
i) Segurança básica (só mapear, sem explorar): fluxo de auth/cookie/JWT, comparação de tokens em tempo constante, limites de corpo/tamanho, path traversal em TempDir/workdir/nomes de arquivo, construção dos argumentos do `docker run` a partir de parâmetros do job (injeção, volumes), segredos em logs

Cada subagente devolve um relatório estruturado, sem implementar, procurando:
- Arquivos > 500 linhas e funções > 80 linhas (top 20 por serviço, com as responsabilidades misturadas); `main.rs`/`lib.rs`/`mod.rs` com lógica
- Handlers com SQL inline ou regra de negócio; `AppState` que virou "god struct"
- Estados como String em vez de enum; transições de estado espalhadas em vez de uma função central
- Números mágicos e literais fora da config (10s, 60min, 96MiB, 600s, nomes de imagem `:local`, paths `/data/*`, portas)
- Mapeamento de erro → HTTP repetido; extractors, paginação e validações duplicados
- Funções com > 5 argumentos, structs com > 15 campos, `pub` em excesso, dependências circulares entre módulos
- Duplicação entre YOLO e difusão (montagem de comando/args, download, report de progresso)
- Mesma lógica implementada em mais de um serviço

# Fase 3 — Consolidação (agente principal)
- Deduplicar entre subagentes e validar por amostragem abrindo os arquivos citados
- Classificar cada item por: categoria, serviço(s), impacto (A/M/B), esforço (P/M/G), risco de regressão, prioridade (P0–P3), tipo (quick win / estrutural), e duas flags: "quebra contrato openapi/protocolo interno?" e "exige deploy coordenado entre serviços?"
- Ordem sugerida: contrato e crates compartilhados (DTOs, erro, config, tracing) → domínio e máquina de estados → camada de repositório → quebrar módulos gigantes → robustez → limpeza
- Toda mudança de protocolo manager↔orchestrator deve propor estratégia de compatibilidade (versionamento ou rollout em duas etapas)

# Entregável
Criar `tasks/backend-modularizacao-auditoria.md`, seguindo as convenções de tasks do AGENTS.md, com:
1. Resumo executivo + métricas por serviço (top arquivos/funções grandes, unwrap/expect fora de testes, nº de duplicações, % de duplicação estimada)
2. Divergências entre a documentação e o código real
3. Arquitetura-alvo por serviço + crates compartilhados propostos (com responsabilidades e o que migra para cada um)
4. Matriz de duplicação entre serviços (o que existe em 2+ lugares e para onde deveria ir)
5. Verificação dos invariantes: cada um respeitado, parcialmente ou violado, com evidência
6. Achados: tabela geral + cada tarefa com ID, evidência (caminho:linha), problema, proposta, critério de aceite (incluindo `cargo check`/`clippy` limpos e contrato inalterado), esforço, risco e dependências
7. Segurança e robustez: seção separada, achados apenas mapeados
8. Roadmap em fases (cada fase = um PR independente e verificável) e o que NÃO mudar
9. Texto sugerido de "Convenções de Backend" para o AGENTS.md (só proposta; não edite o AGENTS.md)

No chat, responda só com os 5 achados mais críticos e o caminho do arquivo.
