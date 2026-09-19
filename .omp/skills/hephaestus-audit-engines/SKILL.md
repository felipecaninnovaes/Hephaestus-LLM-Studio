---
name: hephaestus-audit-engines
description: Auditoria somente leitura das engines Python (engines/engine-kit, trainer-difusao, trainer-yolo, trainer-clip) em modo mock determinístico para padronizar arquitetura, eliminar duplicação entre engines e blindar contratos com o orchestrator. Produz tasks/engines-modularizacao-auditoria.md.
---

# Objetivo
Fazer uma AUDITORIA SOMENTE LEITURA das engines Python do Hephaestus LLM Studio — `engines/engine-kit` (pacote compartilhado), `engines/trainer-difusao`, `engines/trainer-yolo` e `engines/trainer-clip` — para padronizar a arquitetura interna, reduzir arquivos/funções gigantes, eliminar duplicação (principalmente ENTRE engines e o engine-kit) e facilitar a adição de novos modelos/engines.
"Independência e autonomia" significa: adicionar um novo modelo de difusão ou uma nova engine exige só implementar uma interface e registrá-la, sem copiar código de outra engine.

Documentação base: `docs/engines/engine-kit.md`, `docs/engines/trainer-difusao.md`, `docs/engines/trainer-yolo.md`, `docs/engines/trainer-clip.md` — trate como HIPÓTESE. Registre toda divergência entre a documentação e o código real.

# Regras
- NÃO edite código, NÃO reinicie serviços ou containers, NÃO instale nada (pip/uv/poetry), NÃO altere pyproject/requirements/lockfiles/Dockerfiles, NÃO crie venv dentro do repositório.
- NUNCA rode em modo real: use sempre `ENGINE_MOCK=1`. NÃO carregue pesos, NÃO baixe modelos, NÃO use GPU, NÃO toque no Docker socket, S3, banco nem em `/data`. NÃO faça chamadas de rede (inclui APIs de visão) e NÃO leia nem imprima `.env`, chaves ou tokens. Se achar segredo em texto claro no repo, cite só caminho:linha, sem o valor.
- Para não escrever no repositório: `PYTHONDONTWRITEBYTECODE=1`, `ruff --no-cache`, `mypy --cache-dir=/tmp/heph-mypy`, `pytest -p no:cacheprovider --basetemp=/tmp/heph-pytest`. Saídas de ferramentas em /tmp.
- Comandos permitidos: wc/tokei, rg, `python -X importtime` (só engine-kit e módulos leves), `ruff check` (SEM --fix) e `ruff format --check`, mypy/pyright/radon/vulture/deptry se JÁ instalados, `pytest --collect-only`. Só rode a suíte em modo mock se os testes forem herméticos (tmp_path, sem porta fixa como 8090). Ferramenta ausente vira recomendação.
- Não faça `docker build`; leia Dockerfiles como texto.
- Toda afirmação precisa de evidência: caminho:linha e números.
- Só o entregável em `tasks/` pode ser criado.

# Invariantes a preservar (verificar se o código realmente os respeita)
- engine-kit é stdlib-first: nenhum import pesado (torch, diffusers, peft, bitsandbytes, ultralytics, open_clip, transformers) no topo de módulo; lazy imports nas bordas
- Modo mock determinístico sem CUDA; `mock_vector` mantém contrato e semente idênticos ao `MockEmbedder` em Rust (`services/api-principal/src/search/embed.rs`)
- Telemetria atômica: flush por evento, progresso normalizado 0.0–1.0, `telemetry.jsonl` com espelho retrocompatível em `metrics.jsonl`
- `atomic_write` para artefatos, `die()` padronizado, `is_cancelled` checado, `cleanup_cuda()` na limpeza
- VRAM em GiB (1024³) e limites canônicos em `packages/policies/vram-table.yaml`
- Daemons com shutdown gracioso (SIGINT/SIGTERM); daemon de difusão com modelo aquecido e inferência atômica (`_busy`); CLIP com dim 512 e vetores L2-normalizados
- Artefatos mock assinados com `HEPHMOCK`, usados pelo orchestrator para validar o pipeline

# Fase 1 — Reconhecimento (agente principal)
1. Ler AGENTS.md, pyproject/requirements/lockfiles e Dockerfiles de cada engine e do engine-kit, configs de ruff/mypy/pytest, `packages/contracts/openapi.yaml` e `packages/policies/vram-table.yaml`.
2. Inventário quantitativo por pacote: top 30 arquivos .py por linhas, funções > 60 linhas, e contagens de `except Exception`/`except:`, `# type: ignore`, `noqa`, `TODO/FIXME`, `os.environ`, `print(`, `global`, imports pesados no topo de módulo.
3. Descobrir o que a documentação NÃO diz: como o engine-kit chega às engines (pacote instalado, PYTHONPATH ou cópia no Docker), entrypoints e argumentos CLI, formato exato do config, contrato com o orchestrator (args do `docker run`, paths `/data/*`, flag de cancelamento, exit codes), logging, testes e como rodam.
4. Eleger 2–3 "referências de ouro" (os módulos que melhor seguem um bom padrão) como régua.
5. Propor a arquitetura-alvo, partindo do que já existe:
   - engine-kit: infraestrutura sem domínio. Candidatos a entrar: settings/env centralizado, base de validação de config, logging, exit codes, schema de `/health` dos daemons
   - Entrypoints finos (train.py, autolabel.py, autotrack.py, serve.py, server.py): só parse de args e wiring
   - Backends real e mock atrás da MESMA interface (Protocol/ABC), escolhidos em um único ponto (factory), sem `if is_mock()` espalhado
   - Trainers: template method em `BaseModelTrainer` (loop, checkpoint, sample, telemetria, cancelamento no base; só o que difere por modelo nas subclasses)
   - Config tipada (dataclass/pydantic/TypedDict) em vez de dict cru
   - Libs pesadas isoladas em adapters (como `yolo_adapter.py`)

# Fase 2 — Varredura paralela (um subagente por fatia)
a) engine-kit: API pública, coesão dos 6 módulos, custo de import, respeito ao stdlib-first, testes, o que deveria entrar e o que está dentro mas deveria estar fora
b) trainer-difusao — `models/` e `flux_pkg/`: duplicação do loop entre Flux/SDXL/SD15/Mock, o que o `BaseModelTrainer` realmente centraliza, device/dtype/seed, checkpoint e prune, cancelamento, limpeza de VRAM, tamanho dos arquivos
c) trainer-difusao — `common_pkg/`, `generation/`, `serve_pkg/`, `quantization.py`: config, lora_io, text_embeds, encoder_merge; runner/pipelines/text_encoder/progress; estado do daemon (`_busy`, `loaded_spec`), thread-safety, cache de pipelines (memória e descarte), `/shutdown`
d) trainer-yolo: train/autolabel/autotrack, `yolo_adapter.py`, `config.py`, `deterministic.py`, `autolabel_pkg/` (captions, vision_api, pipeline)
e) trainer-clip: server, clip_backend, mock_embed; concorrência do ThreadingHTTPServer com modelo em GPU, batching, limites de payload em `/embed`, dim 512 e normalização L2
f) Duplicação entre engines e engine-kit: em especial `trainer-yolo/deterministic.py` (`_synthetic_metrics`, `_make_fake_artifact`) vs `engine_kit.mock.synthetic_yolo_metrics` e `engine_kit.artifacts.make_fake_artifact`; `trainer-yolo/config.py` vs `trainer-difusao/common_pkg/train_config.py`; `MockTrainer`/`mock_embed.py` vs engine_kit; escrita de telemetria fora do `TelemetryEmitter`; leitura de env espalhada; `die()` vs `raise` vs `sys.exit`; schema de `/health`; Dockerfiles e requirements repetidos
g) Fronteira com orchestrator/api-principal (ler o lado Rust SOMENTE para comparar):
   - args do `docker run`, paths e variáveis de ambiente passados vs esperados pelas engines
   - schema de config vs validadores Python vs `openapi.yaml`: quem é a fonte de verdade, gerado ou manual?
   - campos de `telemetry.jsonl` vs campos consumidos no SSE (`progress`, `phase`, `step`, `epoch`, `vram_used_gb`); alguém ainda lê `metrics.jsonl`?
   - existe teste automatizado de paridade `mock_vector` ↔ `MockEmbedder`?
   - `HEPHMOCK` definido em um só lugar?
   - quem implementa o TTL de inatividade do daemon de difusão (`DIFFUSION_DAEMON_IDLE_TTL_S`): Python ou orchestrator?
   - o padrão de `ENGINE_MOCK` é 1: um deploy real que esqueça `ENGINE_MOCK=0` seria detectado (campo `mode` do `/health`, checagem no orchestrator)?
h) Robustez, testes e tooling: cobertura mock vs real, determinismo/golden tests, type hints da API pública, lints, complexidade, deps sem pin ou duplicadas, Dockerfiles (usuário, camadas, cache do pip) apenas lidos
i) Segurança (só mapear, sem explorar): carregamento de checkpoints enviados por usuários (`.pt` é pickle; `torch.load` com/sem `weights_only`; preferência por safetensors), `yaml.load` vs `safe_load`, `vision_api` (chave, `base_url` configurável, timeouts, logs de payload base64), limites de corpo em `/embed` e `/generate`, path traversal em paths do config e nomes de arquivo, `shell=True`/subprocess, daemons sem autenticação (dependem de rede interna)

Cada subagente devolve um relatório estruturado, sem implementar, procurando:
- Arquivos > 400 linhas e funções > 60 linhas (top 20, com as responsabilidades misturadas); complexidade ciclomática alta
- Imports pesados no topo de módulo e efeitos colaterais em import-time
- `if is_mock()` espalhado em vez de backend/factory único
- Dict cru como config/estado; `os.environ.get` espalhado; números e strings mágicos (paths `/data/*`, portas, model ids, limites de VRAM duplicando o vram-table.yaml)
- `except Exception`/bare except engolindo erro; exit codes inconsistentes
- Loops de treino sem checagem de cancelamento; ausência de try/finally com `cleanup_cuda()`
- Estado global mutável sem lock; escrita não atômica de artefatos e telemetria
- Type hints ausentes na API pública, `type: ignore`/`noqa` em excesso, funções `_privadas` importadas por outros módulos, código morto, imports circulares

# Fase 3 — Consolidação (agente principal)
- Deduplicar entre subagentes e validar por amostragem abrindo os arquivos citados
- Classificar cada item por: categoria, pacote(s), impacto (A/M/B), esforço (P/M/G), risco de regressão, prioridade (P0–P3), tipo (quick win / estrutural), e três flags: "muda o contrato engine↔orchestrator (config, telemetria, args, exit codes)?", "exige rebuild de imagem/deploy coordenado com o orchestrator?" e "afeta a paridade Python↔Rust?"
- Ordem sugerida: engine-kit e settings/config compartilhados → interfaces mock/real → template dos trainers → quebrar módulos gigantes → robustez → limpeza
- Toda mudança de contrato com o orchestrator deve propor estratégia de compatibilidade (ex.: aceitar formato antigo e novo por uma release)

# Entregável
Criar `tasks/engines-modularizacao-auditoria.md`, seguindo as convenções de tasks do AGENTS.md, com:
1. Resumo executivo + métricas por pacote (top arquivos/funções grandes, excepts genéricos, imports pesados no topo, nº de duplicações, % de duplicação estimada)
2. Divergências entre a documentação e o código real
3. Contrato engine↔orchestrator COMO ESTÁ HOJE (config, telemetria, cancelamento, exit codes, args do docker run, paths) e onde ele está duplicado ou implícito
4. Arquitetura-alvo por engine + o que migra para o engine-kit (com responsabilidades)
5. Matriz de duplicação (engine-kit × engines × lado Rust)
6. Verificação dos invariantes: cada um respeitado, parcialmente ou violado, com evidência
7. Achados: tabela geral + cada tarefa com ID, evidência (caminho:linha), problema, proposta, critério de aceite (incluindo ruff/mypy limpos, suíte mock verde e contrato inalterado), esforço, risco e dependências
8. Segurança e robustez: seção separada, achados apenas mapeados
9. Roadmap em fases (cada fase = um PR independente e verificável) e o que NÃO mudar
10. Texto sugerido de "Convenções de Engines" para o AGENTS.md (só proposta; não edite o AGENTS.md)

No chat, responda só com os 5 achados mais críticos e o caminho do arquivo.
