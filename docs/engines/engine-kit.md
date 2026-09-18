# Engine Kit (`engines/engine-kit`)

O `engine-kit` é o pacote Python compartilhado que provê primitivas e utilitários de infraestrutura fundamentais para as engines do Hephaestus (`trainer-difusao`, `trainer-yolo` e `trainer-clip`).

Projetado com filosofia *stdlib-first*, não impõe dependências externas pesadas em tempo de importação. Módulos que dependem de PyTorch ou CUDA utilizam carregamento tardio (*lazy imports*), permitindo que pipelines rodem em modo mock (`ENGINE_MOCK=1`) e em ambientes restritos de CI/CPU com inicialização instantânea e determinismo completo.

---

## Módulos Principais

### 1. `telemetry`
Responsável pela emissão atômica de eventos em tempo real para monitoramento de jobs pelo orchestrator e web UI:
- **`TelemetryEmitter`**: Classe canônica para gerar eventos estruturados em `telemetry.jsonl` (com espelhamento retrocompatível em `metrics.jsonl`).
- Rastreia fases (`phase`), mensagens operacionais, progresso normalizado (`0.0` a `1.0`), passos (`step`/`total_steps`), épocas (`epoch`/`total_epochs`), métricas numéricas arbitrárias e telemetria de VRAM.
- Flush imediato em cada evento emitido para streaming via Server-Sent Events (SSE).

### 2. `mock`
Fornece suporte determinístico para desenvolvimento local sem GPU e testes automatizados:
- **`is_mock(env_val)`**: Avalia de forma resiliente a flag de ambiente `ENGINE_MOCK` (padrão `1`, aceita `true`, `yes`, `1`).
- **`seed_bytes(seed, length)`**: Expande sementes inteiras em fluxos pseudo-aleatórios determinísticos via cadeia SHA-256.
- **`mock_vector(payload, dim)`**: Gera vetores normalizados L2 (dimensão padrão 512), com contrato e semente idênticos ao `MockEmbedder` em Rust (`services/api-principal/src/search/embed.rs`).
- **`synthetic_loss(seed, step, total_steps)`** e **`synthetic_yolo_metrics(...)`**: Simulam curvas realistas de convergência de loss e métricas de precisão.

### 3. `vram`
Gerencia telemetria e higienização de memória gráfica:
- **`vram_allocated_gb()`** e **`vram_reserved_gb()`**: Leitura segura da memória alocada e reservada pelo PyTorch em GiB (divisor canônico $1024^3$).
- **`cleanup_cuda()`**: Força coleta de lixo Python (`gc.collect()`), esvazia cache do allocator CUDA (`empty_cache`) e descarrega pools de memória IPC (`ipc_collect`).
- **`require_cuda(operation_name)`**: Aborta com mensagem descritiva caso operações reais de GPU sejam invocadas sem suporte CUDA.
- *Nota*: As alocações e limites máximos de VRAM são canônicos em `packages/policies/vram-table.yaml`.

### 4. `httpd`
Implementa servidores HTTP daemon para serviços de inferência e busca vetorial:
- **`JSONHandlerMixin`**: Mixin para `BaseHTTPRequestHandler` com utilitários para leitura de payload (`read_json_body`), respostas JSON (`send_json`) e supressão de logs HTTP ruidosos (ativados via `ENGINE_HTTP_DEBUG=1`).
- **`run_daemon(server, name, on_shutdown)`**: Gerencia o ciclo de vida de `ThreadingHTTPServer`, interceptando sinais do sistema operacional (`SIGINT`, `SIGTERM`) para encerramento gracioso e invocação de rotinas de limpeza.

### 5. `artifacts`
Manipulação e retenção de artefatos gerados:
- **`prune_checkpoints(...)`**: Política de rotação de checkpoints intermediários durante o treino para preservar espaço em disco.
- **`make_fake_safetensors(...)`** e **`make_fake_artifact(...)`**: Geração de binários sintéticos simulados com cabeçalhos válidos no modo mock.

### 6. `runtime`
Utilitários de execução e controle de processo:
- **`atomic_write(path, data)`**: Gravação atômica via arquivo temporário para evitar condições de corrida com leitores externos.
- **`die(msg, code)`**: Terminação padronizada de subprocessos com mensagem clara direcionada aos logs do orchestrator.
- **`is_cancelled(path)`**: Checagem de flags de cancelamento emitidas pelo backend.
