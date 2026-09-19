# Levantamento de Backend, Orchestrator, Pipelines e Métricas — Rumo à Autonomia 24/7

Documento de mapeamento, auditoria e checklist de execução de todas as melhorias técnicas necessárias nos subsistemas de **Backend (`api-principal` e `manager`)**, **Orchestrator**, **Pipelines de IA (`engines/*`)**, **Métricas/Observabilidade** e **Resiliência/Autonomia Operacional** do **Hephaestus LLM Studio**.

- **Objetivo:** Permitir que o Hephaestus opere 24/7 de forma contínua, estável, auto-recuperável e independente de intervenções manuais.
- **Data do levantamento:** 2026-09-18.
- **Escopo:** Exclusivamente Backend, Orquestração, Engines, Pipelines e Métricas (zero alterações ou análises de UI/Web).

---

## 1. Backend (`services/api-principal` e `services/manager`)

- [ ] **1.1 [CRÍTICO] Abort em `preparing` deixa o job preso em `cancelling` eternamente e causa re-execução sem pacote**
  - **Severidade:** Crítico
  - **Arquivos:** `services/manager/src/lib.rs:1249-1256`, `services/api-principal/src/jobs/prepare.rs:630-633`, `manager/src/lib.rs:3280-3287`
  - **Problema:** Quando um job em `preparing` é cancelado, o manager muda para `cancelling`. O worker local do BFF detecta o status e apenas atualiza `job_prepares.state = 'cancelled'`, mas **nunca reporta `cancelled` ao manager**. O manager não tem orchestrator associado a esse job, logo ninguém envia report terminal. O job fica preso em `cancelling` permanentemente no dashboard.
  - **Risco 24/7:** No restart do manager, `recover_jobs` reenfileira incondicionalmente jobs em `cancelling` (`status='queued'`), mas como o pacote não foi construído, o job vai para o orquestrador sem `package_ref`, falhando no nó.
  - **Ação:** Criar callback explícito `POST /internal/jobs/:id/prepare-cancel` do BFF para o manager; excluir jobs sem `package_ref` do `recover_jobs`.

- [ ] **1.2 [CRÍTICO] Linha zumbi em `job_prepares` bloqueia permanentemente dedupe do mesmo dataset (503 eterno)**
  - **Severidade:** Crítico
  - **Arquivos:** `services/api-principal/src/jobs/prepare.rs:379-383, 787-852`, migration `0016`
  - **Problema:** `accept_job_preparing` faz `INSERT ... ON CONFLICT (dataset_id, fingerprint) WHERE state = 'preparing' DO NOTHING`. Se o worker travar ou morrer sem panic (ex: I/O hang, kill), a linha fica `preparing` para sempre, pois `recover_stale_prepares` roda **apenas no boot** do `api-principal`. Qualquer submissão futura desse mesmo dataset + fingerprint entra em conflito no INSERT, não encontra o job ativo no re-SELECT (>30min) e devolve `queue_unavailable` (503) em loop até intervenção manual no Postgres.
  - **Risco 24/7:** Paralisação permanente de novos treinos/gerações de um dataset após qualquer instabilidade do processo.
  - **Ação:** Executar `recover_stale_prepares` periodicamente em background (a cada 60s) e expirar `job_prepares` com `updated_at` defasado.

- [ ] **1.3 [CRÍTICO] `MANAGER_TOKEN` com default dev e bind aberto (0.0.0.0:8081)**
  - **Severidade:** Crítico
  - **Arquivos:** `services/manager/src/main.rs:746`, `infra/compose.yaml:31, 63, 89`
  - **Problema:** O manager aceita `MANAGER_TOKEN.unwrap_or_else("manager-dev-token")`. Com a porta `8081` aberta em `0.0.0.0`, qualquer máquina na LAN tem acesso total à API interna de jobs/artifacts/abort.
  - **Risco 24/7:** Falha de segurança grave em deploys de produção ou ambientes corporativos.
  - **Ação:** Fail-fast no boot caso `MANAGER_TOKEN` não esteja definido ou utilize valores triviais (`changeme`, `manager-dev-token`); bind restrito a `127.0.0.1` ou rede interna de containers.

- [ ] **1.4 [ALTO] `prepare_complete` em single-shot: blip de rede destrói pacotes multi-GB**
  - **Severidade:** Alto
  - **Arquivos:** `services/api-principal/src/jobs/prepare.rs:770-782`, `services/api-principal/src/jobs/manager_client.rs:517`
  - **Problema:** Ao concluir o empacotamento, o BFF chama `POST /internal/jobs/:id/prepare-complete`. O cliente HTTP possui timeout de 10s e **zero retries**. Se o manager oscilar por 2s, o BFF assume falha, executa compensação destrutiva (apaga a linha em `dataset_versions` e faz `delete_prefix` no S3 apagando o zip de GBs) e marca o job como `failed`.
  - **Risco 24/7:** Descarte frequente de pacotes pesados gerados com sucesso por oscilações transitórias de rede.
  - **Ação:** Adicionar retry com backoff exponencial (3 a 5 tentativas) no `prepare_complete`/`prepare_fail`.

- [ ] **1.5 [ALTO] Upload chunked de modelos sem TTL/GC (Vazamento de até 8 GiB/sessão)**
  - **Severidade:** Alto
  - **Arquivos:** `services/api-principal/src/models/chunk.rs:13-15, 50-57`
  - **Problema:** Upload chunked aloca sessões em memória (`HashMap`) e arquivos em `TempDir`. Se o cliente fechar a aba ou a conexão cair, os arquivos temporários permanecem em disco indefinidamente até o restart do processo do BFF.
  - **Risco 24/7:** Vazamento progressivo de armazenamento até `ENOSPC` (disco cheio), paralisando o host.
  - **Ação:** Sweeper em background expirando sessões chunked com inatividade > 1h, limpando o `TempDir` associado.

- [ ] **1.6 [ALTO] `gc_dataset_versions` remove linhas no SQL mas não apaga no S3**
  - **Severidade:** Alto
  - **Arquivo:** `services/manager/src/lib.rs:1452-1466`
  - **Problema:** O cleanup de versões de datasets (> 7 dias sem uso) executa `DELETE FROM dataset_versions`, mas como o manager não tem acesso direto ao S3, nenhum `delete_prefix("packages/{version_id}/")` é executado.
  - **Risco 24/7:** Zips multi-GB acumulando sem limite no SeaweedFS, inflando o storage.
  - **Ação:** GC em duas fases: marcar linha deletada e delegar o sweep físico do prefixo S3 ao BFF.

- [ ] **1.7 [ALTO] Abort em `running` via fire-and-forget pode re-executar jobs cancelados**
  - **Severidade:** Alto
  - **Arquivo:** `services/manager/src/lib.rs:1256-1263, 3470-3478`
  - **Problema:** Ao abortar um job em execução, o manager dispara `POST /internal/abort`. Se falhar por timeout de rede (10s), apenas um log `warn` é gerado. Se o orquestrador depois cair ou reiniciar, o watchdog de nó do manager reenfileira o job que estava em `cancelling` (`status='queued'`), re-executando do zero um treino que o usuário cancelou.
  - **Risco 24/7:** Desperdício de horas de GPU contra a intenção do usuário.
  - **Ação:** Fila de retries de abort enquanto o nó estiver vivo; nunca reenfileirar jobs com intenção de cancelamento (`cancelling`).

- [ ] **1.8 [ALTO] `dispatch_next` sem lock atômico: corrida de despacho duplo para o mesmo nó**
  - **Severidade:** Alto
  - **Arquivo:** `services/manager/src/lib.rs:3661-3760`
  - **Problema:** O dispatch valida elegibilidade com `NOT EXISTS (jobs ativos no orquestrador)` e atualiza o job. Se o loop de 2s e o handler de `prepare-complete` executarem concorrentemente, ambos podem ver o nó livre e despachar dois treinos pesados para a mesma GPU.
  - **Risco 24/7:** Colisão de processos GPU e estouro imediato de VRAM (CUDA OOM).
  - **Ação:** Utilizar `SELECT ... FOR UPDATE SKIP LOCKED` e lock advisory/distribuído por nó durante a decisão de despacho.

- [ ] **1.9 [MÉDIO] `report_job` em `done` sem transação SQL atômica**
  - **Severidade:** Médio
  - **Arquivo:** `services/manager/src/lib.rs:1673-1700`
  - **Problema:** DELETE de artefatos, múltiplos INSERTs e hooks em queries separadas fora de transação. Falha intermediária deixa o job como `done` mas com metadados/galeria corrompidos.
  - **Ação:** Envolver operações de conclusão de job em transação `pool.begin()`.

- [ ] **1.10 [MÉDIO] SSE de eventos de job via polling cego sem cache**
  - **Severidade:** Médio
  - **Arquivo:** `services/api-principal/src/jobs/handlers.rs:516-600`
  - **Problema:** Polling a cada 1s contra o manager por cliente conectado. Conexões zumbis (jobs em `cancelling`) geram carga contínua no manager.
  - **Ação:** Cache de snapshot com ETag ou canal pub/sub interno.

- [ ] **1.11 [MÉDIO] `get_artifact_data` carrega objetos multi-GB inteiros em RAM**
  - **Severidade:** Médio
  - **Arquivo:** `services/api-principal/src/jobs/handlers.rs:664-712`
  - **Problema:** `storage.get` lê o arquivo inteiro em buffer na memória para validar MD5, arriscando OOM do BFF em downloads concorrentes de modelos de 8 GB.
  - **Ação:** Streaming direto para o cliente HTTP + validação de hash em chunk.

- [ ] **1.12 [MÉDIO] Watchdog de prepare ancorado em `created_at`**
  - **Severidade:** Médio
  - **Arquivo:** `services/manager/src/lib.rs:1424-1439`
  - **Problema:** Timeout de 60min mata empacotamentos legítimos que levam mais de uma hora, ignorando o heartbeat `updated_at` de `job_prepares`.
  - **Ação:** Ancorar watchdog no `updated_at` do worker.

---

## 2. Orchestrator (`services/orchestrator`)

- [ ] **2.1 [CRÍTICO] Ausência de semáforo local de GPU/VRAM por nó**
  - **Severidade:** Crítico
  - **Arquivos:** `services/orchestrator/src/main.rs:156-213`, `lib.rs:1938-1974`
  - **Problema:** O `dispatch_handler` aceita qualquer requisição HTTP do manager e executa `tokio::spawn(run_job(...))` imediatamente, confiando cegamente no manager. Se houver descompasso ou retry, múltiplos containers sobem na mesma GPU física.
  - **Risco 24/7:** Crash catastrófico de drivers CUDA e OOM generalizado.
  - **Ação:** Semáforo local estrito por GPU/Engine rejeitando com HTTP 503 (`node_busy`) caso a GPU já esteja ocupada.

- [ ] **2.2 [CRÍTICO] Executor de containers sem timeout de processo e sem `kill_on_drop`**
  - **Severidade:** Crítico
  - **Arquivo:** `services/orchestrator/src/lib.rs:1032-1081`
  - **Problema:** O container de treino é aguardado via `cmd.output().await` sem nenhum timeout configurável e sem `kill_on_drop(true)` no `tokio::process::Command`. Se a engine travar em kernel hang da GPU, deadlock de Python ou espera de socket, o nó fica bloqueado indefinidamente.
  - **Risco 24/7:** Nós de computação permanentemente inutilizados até intervenção manual via SSH.
  - **Ação:** Timeout global por tipo de job + garantia de `kill_on_drop` no processo executor.

- [ ] **2.3 [CRÍTICO] `is_cancelled()` é código morto durante staging S3 e uploads**
  - **Severidade:** Crítico
  - **Arquivos:** `services/orchestrator/src/lib.rs:1124`, `main.rs:246-252`
  - **Problema:** Ao receber abort, o orquestrador apenas seta uma flag e faz `docker stop <trainer-...>`. Durante o download do dataset do S3 (que pode durar minutos), montagem de pesos ou upload de artefatos finais, a flag nunca é consultada. No caminho do daemon quente (`diffusion-daemon`), o `docker stop` erra o nome do container e a geração continua até o fim.
  - **Risco 24/7:** Desperdício contínuo de banda e ciclos de GPU após cancelamentos.
  - **Ação:** Checar `is_cancelled()` a cada chunk de I/O; enviar sinal de cancelamento ao daemon; garantir deleção de `tmp/<job_id>`.

- [ ] **2.4 [CRÍTICO] Containers de treino órfãos pós-crash do orquestrador (Sem sweep no boot)**
  - **Severidade:** Crítico
  - **Arquivo:** `services/orchestrator/src/lib.rs:1908, 1938`
  - **Problema:** Containers de treino são lançados com `docker run --rm --name trainer-{engine}-{job_id}`. Se o processo do orquestrador reiniciar ou sofrer crash, o container continua rodando na máquina como órfão consumindo 100% da GPU. O novo processo do orquestrador sobe com a memória limpa e não faz varredura de containers existentes.
  - **Risco 24/7:** GPUs permanentemente ocupadas por processos fantasmas.
  - **Ação:** No boot do orquestrador, executar `docker ps --filter name=trainer-` e forçar `docker stop/rm`.

- [ ] **2.5 [ALTO] Ausência de tratamento de `SIGTERM` no encerramento do orquestrador**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/main.rs:347-667`
  - **Problema:** O servidor Axum é encerrado abruptamente pelo Docker (`SIGTERM` vira `SIGKILL`). Nenhuma rotina intercepta o sinal para notificar o manager sobre falha dos jobs ativos ou parar graciosamente os containers filhos.
  - **Risco 24/7:** Jobs ficam presos em `running` no manager até o timeout do watchdog.
  - **Ação:** Interceptar `tokio::signal::unix::SignalKind::terminate()`, marcar jobs locais como failed/interrompidos no manager e emitir `docker stop` nos containers filhos.

- [ ] **2.6 [ALTO] Stdout/Stderr de treinos acumulados inteiramente em memória**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/lib.rs:1032-1081`
  - **Problema:** A saída completa do container (horas de logs de épocas) é capturada em uma única `String` via `cmd.output()`. Em treinos longos, isso consome centenas de megabytes em RAM desnecessariamente.
  - **Risco 24/7:** Risco de OOM do próprio processo orquestrador.
  - **Ação:** Streaming contínuo de logs para arquivo rotativo em disco, retendo apenas as últimas N linhas em memória.

- [ ] **2.7 [ALTO] Vazamento de disco em `datasets-cache/` e `outputs/`**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/lib.rs:1250-1262, 2656-2657`
  - **Problema:** A cada job, o pacote baixado é descompactado em `datasets/datasets-cache/{job_id}` e os artefatos são gerados em `outputs/{job_id}`. O código apenas remove `tmp/{job_id}` após o sucesso.
  - **Risco 24/7:** Exaustão completa de disco no host ao longo de semanas de operação.
  - **Ação:** Limpeza obrigatória pós-job dos diretórios de cache do dataset e outputs já enviados ao S3, com política de LRU no boot.

- [ ] **2.8 [ALTO] Reports de telemetria e progresso sem retry (Perda silenciosa)**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/lib.rs:864-911, 1195-1206`
  - **Problema:** O `HttpReportClient` possui timeout de 10s e **zero retries**. Se o manager reiniciar no instante em que o treino termina, o report de `done` é descartado silenciosamente (`let _ =`). O orquestrador fecha o container e o job fica marcado como `running` para sempre no manager.
  - **Risco 24/7:** Jobs finalizados com sucesso ficam presos em `running` eternamente.
  - **Ação:** Fila local persistente em memória/disco com retry exponencial até confirmação do manager (ACK).

- [ ] **2.9 [ALTO] Concorrência no spawn do Daemon de Difusão e estado fantasma pós-OOM**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/daemon.rs:481-537`
  - **Problema:** Se duas gerações chegam simultaneamente com o daemon desligado, ambas passam pela checagem `!is_running()` e tentam subir containers concorrentes; o comando `docker rm -f` de uma mata a outra. Se o daemon sofre OOM kill pelo kernel, a flag `is_running` permanece `true`, gerando timeouts de 60s em cada request seguinte.
  - **Ação:** Mutex de inicialização (single-flight) no daemon e probe de liveness ativo antes de despachar cada inferência.

- [ ] **2.10 [MÉDIO] SDK S3 do orquestrador com `RetryConfig::disabled()`**
  - **Severidade:** Médio
  - **Arquivo:** `services/orchestrator/src/lib.rs:788`
  - **Problema:** Desliga retries em downloads de pesos e datasets, falhando treinos de horas por pequenas falhas transitórias de conexão com o SeaweedFS.
  - **Ação:** Ativar retry padrão do AWS SDK para operações de leitura.

---

## 3. Pipelines e Engines de IA (`engines/*`)

- [ ] **3.1 [CRÍTICO] `torchao` ausente na imagem GPU: treinos 2bit e 6bit sempre falham**
  - **Severidade:** Crítico
  - **Arquivos:** `engines/trainer-difusao/Dockerfile.gpu:10-20`, `quantization.py:36-42`, `flux.py:504-512`
  - **Problema:** O contrato OpenAPI e o código Python suportam quantizações `2bit` e `6bit` via `torchao.dtypes.IntxWeightOnlyConfig`. Porém, o `Dockerfile.gpu` **não instala `torchao`**. Quando um usuário agenda um treino de FLUX com 2bit/6bit, o sistema faz todo o staging e download de dezenas de GBs de dados para, em seguida, disparar `_die()` na engine logo antes de começar a treinar.
  - **Risco 24/7:** Impossibilidade de uso de quantizações avançadas e falha garantida pós-staging.
  - **Ação:** Adicionar `torchao` (e `prodigyopt`) às dependências instaladas no `Dockerfile.gpu`.

- [ ] **3.2 [CRÍTICO] Divergência de dependências: `uv.lock` (Torch 2.14) vs `Dockerfile.gpu` (Torch 2.6)**
  - **Severidade:** Crítico
  - **Arquivos:** `engines/trainer-difusao/pyproject.toml`, `uv.lock`, `Dockerfile.gpu:1`
  - **Problema:** O `uv.lock` foi resolvido e testado com PyTorch 2.14+, `diffusers 0.40.0` e `torchao 0.18.0`. Porém, o `Dockerfile.gpu` baseia-se em `pytorch/pytorch:2.6.0-cuda12.4` e roda `pip install` solto, ignorando o lockfile. No PyTorch 2.6, tipos como `torch.int2` não existem.
  - **Risco 24/7:** O código validado localmente não reflete o comportamento do nó GPU em produção.
  - **Ação:** Sincronizar o ambiente Docker para respeitar o `uv.lock` com suporte CUDA formalizado.

- [ ] **3.3 [CRÍTICO] Deadlock fatal no handler de `SIGTERM` do Daemon de Difusão**
  - **Severidade:** Crítico
  - **Arquivo:** `engines/trainer-difusao/src/trainer_difusao/serve.py:392-409`
  - **Problema:** O signal handler de `SIGTERM` chama diretamente `_do_shutdown_graceful()`. Essa função invoca `ThreadingHTTPServer.shutdown()`, que bloqueia até que a thread principal saia de `serve_forever()`. Como o signal handler está rodando exatamente na thread principal, a execução entra em deadlock. O container nunca desliga graciosamente e só é encerrado após 5 segundos com `SIGKILL` do kernel.
  - **Risco 24/7:** Vazamento de VRAM e encerramentos abruptos corrompendo o estado da GPU.
  - **Ação:** Disparar o desligamento em uma thread separada (`threading.Thread(target=_do_shutdown_graceful, daemon=True).start()`).

- [ ] **3.4 [ALTO] Zero tratamento de CUDA OOM nos loops de treino e sem limpeza de VRAM**
  - **Severidade:** Alto
  - **Arquivos:** `engines/trainer-difusao/src/trainer_difusao/models/` (`sdxl.py:455+`, `flux.py:1327+`, `sd15.py:395+`)
  - **Problema:** Nenhum loop de treino captura `torch.cuda.OutOfMemoryError`. Se a GPU esgotar VRAM em épocas avançadas, a engine quebra com traceback cru, o processo morre com exit code 1 sem emitir evento estruturado de erro na telemetria, e o treino inteiro é perdido. Não há chamadas periódicas a `torch.cuda.empty_cache()` ou `gc.collect()`.
  - **Risco 24/7:** Treinos de várias horas perdidos por fragmentação de allocator CUDA.
  - **Ação:** Envolver iterações de treino em bloco `try/except torch.OutOfMemoryError`, emitir telemetria clara de OOM antes de falhar, e invocar limpeza explícita de VRAM ao final de cada época/job.

- [ ] **3.5 [ALTO] Checkpoints gerados a cada época sem retenção (Disco explode)**
  - **Severidade:** Alto
  - **Arquivos:** `models/sdxl.py:575-590`, `models/flux.py:1505-1522`, `models/sd15.py:~540`
  - **Problema:** A engine salva `checkpoints/<base>_epoch_XXX.safetensors` a cada época (intervalo default = 1). Um treino de 50 épocas de FLUX LoRA gera dezenas de GBs de arquivos intermediários sem nenhuma política de retenção (`keep_last_n`).
  - **Risco 24/7:** Falha por disco cheio (`ENOSPC`) no volume do container durante treinos longos.
  - **Ação:** Implementar retenção automática local (manter apenas os últimos 2 checkpoints e o melhor `best.safetensors`).

- [ ] **3.6 [ALTO] Fallbacks silenciosos que violam a política "Honesto, nunca degrada"**
  - **Severidade:** Alto
  - **Arquivos:** `generate.py:1180-1196`, `optimizers.py:30-49`
  - **Problema:** Se BitsAndBytes (4bit/8bit) falhar na geração, chaveia para precisão total silenciosamente (VRAM muito maior, OOM provável). Se o otimizador `prodigy` falhar (ausente na imagem), chaveia silenciosamente para `AdamW`.
  - **Ação:** Falhar de forma honesta e explícita (`_die()`) em vez de degradar para configurações não solicitadas.

- [ ] **3.7 [ALTO] Crash de treino não emite telemetria de erro para o orquestrador**
  - **Severidade:** Alto
  - **Arquivos:** `engines/trainer-difusao/src/trainer_difusao/train.py:85-91`, `engines/trainer-yolo/src/trainer_yolo/train.py:403-412`
  - **Problema:** `trainer.train(...)` é chamado sem bloco `try/except`. Qualquer erro inesperado fecha o processo sem registrar a linha com `phase="error"` em `metrics.jsonl`/`telemetry.jsonl`.
  - **Ação:** Capturar exceções não tratadas no entrypoint da engine e forçar o flush de um evento estruturado de erro antes de propagar o encerramento.

- [ ] **3.8 [MÉDIO] Yolo Mock só escreve `metrics.jsonl` após o fim do treino**
  - **Severidade:** Médio
  - **Arquivo:** `train.py:174-178`
  - **Problema:** Acumula métricas em RAM e só descarrega o arquivo ao final, impedindo o teste de streaming progressivo durante simulações mock.
  - **Ação:** Abrir e descarregar linha a linha a cada época com `flush()`.

- [ ] **3.9 [MÉDIO] Falha em imagem isolada derruba treino inteiro no dataset**
  - **Severidade:** Médio
  - **Arquivo:** `dataset.py:113-119`
  - **Problema:** Não trata erros de `PIL.Image.open()`. Uma única imagem corrompida no dataset causa exceção e aborta o treino imediatamente.
  - **Ação:** Pré-flight com `PIL.Image.verify()` no `__init__`, pulando ou reportando imagens ruins em métricas.

---

## 4. Métricas, Telemetria & Observabilidade

- [ ] **4.1 [CRÍTICO] Ausência total de instrumentação Prometheus (`/metrics`) nos serviços core**
  - **Severidade:** Crítico
  - **Arquivos:** `services/api-principal`, `services/manager`, `services/orchestrator`
  - **Problema:** Nenhum dos três serviços Rust exporta métricas em formato Prometheus/OpenMetrics. Não há visibilidade sobre:
    - Uso e esgotamento do pool de conexões com o Postgres (`sqlx::PgPool`).
    - Latência de handlers e contagem de status HTTP (2xx, 4xx, 5xx).
    - Profundidade da fila de jobs, tempo médio de espera e taxa de jobs abortados/falhados.
    - Saturação de requests e taxas de erro nas chamadas ao S3 (SeaweedFS).
    - Ticks dos loops de watchdog e workers em background.
  - **Risco 24/7:** Cegueira operacional absoluta diante de lentidões, vazamentos de conexões e travamentos de background.
  - **Ação:** Integrar `metrics` e `metrics-exporter-prometheus` expondo `/metrics` (protegido ou em porta interna de telemetria) nos três serviços.

- [ ] **4.2 [ALTO] Quebra na correlação distribuída de logs (`x-request-id` / TraceContext)**
  - **Severidade:** Alto
  - **Arquivos:** `api-principal/src/jobs/manager_client.rs`, `manager/src/lib.rs` (`HttpOrchestratorClient`), `orchestrator/src/lib.rs`
  - **Problema:** O `api-principal` não repassa `x-request-id` nas chamadas HTTP ao `manager`. O `manager` gera novas requisições para o `orchestrator` sem contexto de rastreio. Em caso de falha de um job, é impossível correlacionar os logs do BFF, do Manager e do Orquestrador sem busca manual.
  - **Ação:** Propagar headers padrão `x-request-id` e W3C `traceparent` em todas as chamadas HTTP internas da malha de serviços.

- [ ] **4.3 [ALTO] Falta de monitoramento de integridade de hardware GPU (Xid / ECC / Travamentos)**
  - **Severidade:** Alto
  - **Arquivo:** `services/orchestrator/src/main.rs:624-647`
  - **Problema:** O orquestrador apenas executa parsing simples de memória e utilização via `nvidia-smi`. Ele não detecta eventos críticos de hardware como GPU caída do barramento PCIe, erros Xid no `dmesg`, GPU em throttling severo ou travada em estado D.
  - **Ação:** Auditoria básica de integridade de GPU no heartbeat do orquestrador; marcar o nó como `unhealthy`/`degraded` caso a GPU responda com erro estrutural.

- [ ] **4.4 [ALTO] Probes de liveness e readiness rasas (`/health` vs `/ready`)**
  - **Severidade:** Alto
  - **Arquivos:** `services/api-principal/src/monitoring.rs`, `services/manager/src/main.rs:170`, `services/orchestrator/src/main.rs:136`
  - **Problema:** No compose, os serviços usam dependências baseadas em `service_started` em vez de `service_healthy`. O `/health` responde 200 OK sem checar se as dependências downstream (Postgres, S3, Docker daemon) estão de fato operantes.
  - **Ação:** Separar `/health` (liveness) de `/ready` (readiness: consegue falar com Postgres, S3 e Docker socket); alinhar o `compose.yaml` para usar `condition: service_healthy`.

- [ ] **4.5 [MÉDIO] Exposição de senhas de bootstrap em nível WARN**
  - **Severidade:** Médio
  - **Arquivo:** `api-principal/src/main.rs:~200`
  - **Problema:** A senha gerada no primeiro boot é impressa nos logs em nível `warn`. Em agregadores de log de produção, credenciais ficam persistidas em texto plano.
  - **Ação:** Mascarar ou restringir a saída a console interativo local.

- [ ] **4.6 [MÉDIO] Ausência de rotação de logs nos containers**
  - **Severidade:** Médio
  - **Arquivo:** `infra/compose.yaml`
  - **Problema:** Nenhum serviço no `compose.yaml` define diretivas de `logging: { options: { max-size, max-file } }`. O driver `json-file` padrão acumula logs indefinidamente no disco do host.
  - **Ação:** Adicionar âncora `x-logging` com limite de 10 MB e 3 arquivos de retenção.

---

## 5. Resiliência Operacional, Garbage Collection & Infraestrutura

- [ ] **5.1 [CRÍTICO] Inexistência total de rotina de Backup e Disaster Recovery para Postgres e SeaweedFS**
  - **Severidade:** Crítico
  - **Arquivos:** `infra/compose.yaml`, `infra/seaweedfs-s3.json`, `scripts/`
  - **Problema:** Não existe nenhum script, container sidecar ou cron configurado para `pg_dump` do Postgres ou replicação dos volumes do SeaweedFS. Um comando `docker compose down -v` acidental ou corrupção de sistema de arquivos elimina completamente o banco e todos os dados binários.
  - **Risco 24/7:** Perda catastrófica e definitiva de dados de usuários, anotações, embeddings e modelos treinados.
  - **Ação:** Sidecar de backup agendado (ex: a cada 6h) executando `pg_dump` e espelhando metadados e blobs para armazenamento externo/segundo volume, com runbook de restauração testado.

- [ ] **5.2 [CRÍTICO] Ausência de `restart: unless-stopped` nos serviços principais do Compose**
  - **Severidade:** Crítico
  - **Arquivo:** `infra/compose.yaml:10-180`
  - **Problema:** Nenhum serviço essencial (`db`, `principal`, `manager`, `seaweedfs`, `orchestrator-local`, `embedder`) possui política de reinicialização (`restart:`). Se qualquer serviço sofrer crash pontual ou se o host Linux for reiniciado, a aplicação inteira permanece morta até intervenção manual.
  - **Risco 24/7:** Indisponibilidade crônica após reinicializações do servidor host.
  - **Ação:** Configurar `restart: unless-stopped` em todos os serviços long-running do Compose.

- [ ] **5.3 [CRÍTICO] Job zumbi quando orquestrador finaliza mas report falha (Manager não grava)**
  - **Severidade:** Crítico
  - **Arquivo:** `services/manager/src/lib.rs:3446-3508`
  - **Problema:** O watchdog só reenfileira por nó offline (heartbeat > 60s). Se o orquestrador segue vivo mas o report final falhou, o job fica `running` eternamente no banco, pois não há timeout por job.
  - **Risco 24/7:** Fila travada e relatórios incorretos.
  - **Ação:** Watchdog por job: status `running` sem telemetria por tempo excessivo (> 2x o intervalo de telemetria) transiciona para falha/requeue.

- [ ] **5.4 [ALTO] Lixeira de datasets sem TTL purge agendado**
  - **Severidade:** Alto
  - **Arquivo:** `services/api-principal/src/datasets/handlers.rs:336, 498-532`
  - **Problema:** Soft delete usa `images.deleted_at` e a limpeza é puramente manual via UI (`DELETE /api/datasets/:id/trash`).
  - **Risco 24/7:** Acúmulo infinito de imagens deletadas consumindo S3.
  - **Ação:** Sweeper no manager (ex: a cada 1h) com purge físico de imagens com `deleted_at > TRASH_TTL_DAYS` (default 30).

- [ ] **5.5 [ALTO] Cleanup de jobs antigos apenas sob demanda**
  - **Severidade:** Alto
  - **Arquivo:** `services/manager/src/lib.rs:2916-2986`
  - **Problema:** `cleanup_jobs` só é disparado quando o usuário clica no modal da UI. As tabelas de `jobs`, `job_artifacts`, métricas em JSONB e logs crescem sem teto no Postgres.
  - **Ação:** Agendar execução de `cleanup_jobs` para jobs terminais com mais de 90 dias no worker do manager.

- [ ] **5.6 [ALTO] Inputs avulsos de Img2Img (`generation_inputs`) sem rotina de expurgo**
  - **Severidade:** Alto
  - **Arquivos:** `services/api-principal/migrations/0017_generation_inputs.sql`, `generations/inputs.rs:31-32`
  - **Problema:** A tabela marca `used_at`, mas os arquivos em `generation_inputs/{id}/` no S3 nunca são varridos nem deletados.
  - **Ação:** Criar rotina periódica de deleção de objetos em `generation_inputs` consumidos há mais de N dias.

- [ ] **5.7 [ALTO] Reconciliação Storage S3 vs Banco de Dados**
  - **Severidade:** Alto
  - **Arquivo:** `services/api-principal/src/storage/s3.rs`
  - **Problema:** O `StoragePort` não possui método de listagem (`ListObjectsV2`). Todas as compensações pós-falha são best-effort; sweeps que falham deixam objetos fantasmas no S3 para sempre.
  - **Ação:** Job semanal de reconciliação cruzando objetos sob prefixos S3 com registros válidos no banco de dados.

- [ ] **5.8 [ALTO] SeaweedFS sem compactação automática (Vacuum)**
  - **Severidade:** Alto
  - **Arquivo:** `infra/compose.yaml:45-55`
  - **Problema:** Roda sem flags de threshold de lixo (`-garbageThreshold=0.3`). Deletes apenas marcam metadados; o espaço físico no disco do host só é recuperado após vacuum manual.
  - **Ação:** Configurar `-garbageThreshold=0.3` e rotina periódica de vacuum no master.

---

## 6. Roteiro de Execução Prioritária (4 Fases)

### Fase 1: Sobrevivência Básica e Blindagem Operacional (Dias 1–3)
- [ ] Implementar `restart: unless-stopped` e `condition: service_healthy` no `compose.yaml` (Item 5.2).
- [ ] Fechar portas do Postgres, Manager e Orchestrator no host (Item 1.3).
- [ ] Implementar container sidecar de backup automatizado (`pg_dump`) (Item 5.1).
- [ ] Adicionar `torchao` e alinhar PyTorch no `Dockerfile.gpu` (Itens 3.1 e 3.2).
- [ ] Corrigir deadlock de `SIGTERM` no Daemon de Difusão (Item 3.3).

### Fase 2: Garbage Collection e Estancamento de Vazamentos (Dias 4–6)
- [ ] Implementar TTL e expurgo automático de sessões chunked em memória/disco (Item 1.5).
- [ ] Implementar limpeza obrigatória pós-job de `datasets-cache/` e `outputs/` no orquestrador (Item 2.7).
- [ ] Implementar GC físico no S3 ao deletar `dataset_versions` (Item 1.6) e `generation_inputs` (Item 5.6).
- [ ] Implementar retenção de checkpoints por época nas engines de treino (Item 3.5).
- [ ] Configurar vacuum automático no SeaweedFS (Item 5.8).

### Fase 3: Confiabilidade de Fila, Concorrência e Resiliência (Dias 7–9)
- [ ] Corrigir máquina de estados para abort em `preparing` e loop de recovery do dedupe (Itens 1.1 e 1.2).
- [ ] Implementar semáforo local de concorrência de GPU no orquestrador (Item 2.1).
- [ ] Implementar timeout e `kill_on_drop` nos executores de containers (Item 2.2).
- [ ] Implementar varredura e limpeza de containers `trainer-*` órfãos no boot do orquestrador (Item 2.4).
- [ ] Adicionar retry com backoff exponencial no `prepare_complete` (Item 1.4) e nos reports do orquestrador (Item 2.8).
- [ ] Capturar CUDA OOM e emitir telemetria estruturada antes de encerrar containers (Itens 3.4 e 3.7).

### Fase 4: Observabilidade e Prontidão de Produção (Dias 10–12)
- [ ] Implementar endpoint `/metrics` padrão Prometheus no `api-principal`, `manager` e `orchestrator` (Item 4.1).
- [ ] Propagar `x-request-id` e `traceparent` nas chamadas internas da malha (Item 4.2).
- [ ] Implementar auditoria de integridade física e erros Xid da GPU no heartbeat (Item 4.3).
- [ ] Configurar segmentação de redes Docker em 3 zonas e limites de memória por container.
