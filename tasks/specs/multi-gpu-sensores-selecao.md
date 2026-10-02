# Spec — Sensores de GPU e Seleção Multi-GPU

**Data:** 2026-10-02 (revisão; substitui a versão de 2026-09-25)
**Autor:** @orchestrator (plano do @planner + decisões do usuário)
**Status:** Aprovada — decisões do usuário 2026-10-02
**Nó de referência:** VM `docker-04` (`10.15.50.114`)
- GPU0 NVIDIA GeForce RTX 3060 12 GB — `GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17`
- GPU1 NVIDIA GeForce GTX 1660 Super 6 GB — `GPU-c83cc056-07f7-d31e-cc98-7486ddac0296`

Hoje `ORCH_GPU_DEVICES=0`: tudo roda na 3060. `nvidia-smi` na VM reporta W/%/°C.

---

## 1. Contexto

A telemetria de GPU é agregada por nó (`vram_used`/`vram_total` somados, `gpus: Vec<String>` só com nomes). O job não sabe em que placa roda: o container sobe com `--gpus "device={ORCH_GPU_DEVICES}"` fixo do boot. O daemon de difusão é preemptado incondicionalmente antes de qualquer job, mesmo que o job pudesse rodar na outra placa. A eleição compara a VRAM do nó, não a de uma GPU.

Objetivos:
1. Sensores por GPU física (VRAM, W, %, °C) no heartbeat, na API e na UI.
2. GPU escolhível por job, com escolha automática por VRAM e identificador estável (UUID — Pitfall D9).
3. Daemon de difusão fixado numa GPU; jobs em outra GPU não o derrubam.
4. Métricas `sys.gpu.*` por job e alerta `vram_high`.

---

## 2. Decisões

### 2.1 Usuário (2026-10-02)
1. **1 job por nó.** `select_eligible_orchestrator` (`services/manager/src/dispatch/election.rs:37-39`) continua bloqueando o nó com job `dispatched|running|cancelling`. Sem concorrência por GPU.
2. **GPU escolhível por job** (`gpuDevice`). Sem `gpuDevice`: **menor GPU cujo `vram_total` (MiB) ≥ required_gb** da vram-table; empate → menor `vram_used`; required desconhecido → maior GPU.
3. **Daemon de difusão fixado** via `DIFFUSION_DAEMON_GPU_DEVICE` (UUID; no docker-04 = 3060). `maybe_preempt_daemon` (`services/orchestrator/src/daemon/lifecycle.rs:70-91`) só derruba o daemon se a GPU do job == GPU do daemon; se uma das duas for desconhecida, mantém o comportamento atual (derruba).
4. **GPU manual abaixo do mínimo da vram-table → HTTP 400 `insufficient_gpu_vram`**; a UI desabilita a placa.

### 2.2 Orchestrator
- Identificador canônico = **UUID** (`GPU-…`). Índice (`"0"`, `"1"`) aceito só na entrada; o manager resolve para UUID no create pelo `TelemetryCache` do nó e grava o UUID. Formato: `^GPU-[0-9a-fA-F-]{8,60}$` ou `^[0-9]{1,2}$`, ≤64 chars; senão 400 `invalid_gpu_device`.
- `gpuDevice` exige `orchestratorId` → senão 400 `gpu_device_requires_orchestrator`. GPU inexistente no nó → 400 `unknown_gpu_device`.
- Persistência: **`jobs.gpu_device TEXT NULL`** = GPU efetiva (UUID). Manual: gravada no create. Automática: gravada pela eleição no dispatch. Requeue mantém a manual; a automática é re-escolhida.
- Eleição usa **VRAM por GPU** (`gpu_devices` do nó; fallback `max_gpu_mib`/comportamento atual se o nó não reporta `gpu_devices`), nunca a soma do nó. O implementador confirma o que `orchestrators.vram_total_gb` guarda hoje (soma vs maior placa) e cita file:line.
- Nó sem `gpu_devices` (mock/CPU/legado): `gpu_device` NULL, dispatch sem o campo → orchestrator usa `ORCH_GPU_DEVICES` (fallback atual).
- Executor: `--gpus "device=<UUID>"` (fallback `ORCH_GPU_DEVICES`). Não duplicar com `NVIDIA_VISIBLE_DEVICES` se `--gpus` já resolve (implementador verifica; uma fonte só).
- **Sampler único** no orchestrator (§5.1).
- `orchestrators.gpu_devices JSONB NULL`; coluna legada `gpus` mantida.
- Sem script de smoke permanente; o smoke é execução real conduzida pelo orchestrator.

---

## 3. Delta: spec de 2026-09-25 × código atual (`develop` @ `79922c2`)

| Caminho / símbolo | Onde (file:line) | Hoje | Muda |
|---|---|---|---|
| `services/orchestrator/src/telemetry/gpu.rs` | `:1-84` | `GpuTelemetry` agrega `gpus`, `vram_total`, `vram_used`, `max_gpu_mib`; `nvidia-smi --query-gpu=name,memory.total,memory.used`, timeout 2s. Sem index/uuid/W/%/°C. | Sampler único por placa com cache compartilhado (§5.1). |
| `adapters/executor_docker.rs` | `:208,236-241,271` | `build_docker_run_args(gpu_devices: Option<&str>)` → `--gpus "device=…"` + `--shm-size 2g`, valor estático `ORCH_GPU_DEVICES`. | Recebe `dispatch.gpu_device` (UUID), fallback `ORCH_GPU_DEVICES`. |
| `daemon/launcher.rs` | `:26,39,48,88-91` | `DockerDaemonLauncher.gpu_devices` estático do boot. | Usa `DIFFUSION_DAEMON_GPU_DEVICE`. |
| `maybe_preempt_daemon` | `daemon/lifecycle.rs:70-91`; chamado em `app/mod.rs:252` | Derruba o daemon ocioso antes de qualquer job. | Recebe a GPU alvo; só derruba se igual à do daemon (decisão 3). |
| `services/manager/src/nodes/heartbeat.rs` | `:56-73,84-88` | Grava `gpus` e `vram_total_gb` em `orchestrators`; atualiza `TelemetryCache`. | Recebe/persiste `gpu_devices`; popula o cache. |
| `services/manager/src/nodes/cache.rs` | `:8-23` | `TelemetryState` sem dados por placa. | `gpu_devices: Vec<GpuDeviceTelemetry>`. |
| `dispatch/election.rs` | `:1-101` | `FOR UPDATE OF o`, `status='online'`, sem job ativo, `vram_total_gb >= required_gb`; `hint` de nó. Não conhece GPUs. | Mantém 1 job/nó; escolhe/valida GPU por VRAM da placa. |
| `DispatchPayload` / `DispatchRequest` | `services/manager/src/dispatch/payload.rs:43-80`; `crates/heph-contracts/src/dispatch.rs:46-68`; `services/orchestrator/src/domain/models.rs:3-6` | Sem GPU. (`JobDispatchPayload` da spec antiga não existe.) | `gpu_device: Option<String>`. |
| `ORCH_GPU_DEVICES` | `services/orchestrator/src/config/mod.rs:50,142`; `main.rs:99,190,228` | Modo GPU + device padrão. | Fallback para nós legados/job sem GPU. |
| `orchestrators.gpus` | `services/api-principal/migrations/0006_jobs.sql:10` | JSONB com nomes. | Mantida; nova `gpu_devices JSONB NULL`. |
| `services/api-principal/src/monitoring.rs` | `:27-56,102-120` | `OrchestratorResponse` com `gpus`, VRAM agregada, disco. | `gpuDevices`. |
| `jobs/handlers/stream.rs` | `:280-331` | `get_job_metrics`; `get_telemetry` (`TelemetryResponse`). | `gpuDevices` na telemetria; `sys.gpu.*` servidos pela query existente. |
| `jobs/models.rs` | `:40-70,120+` | 5 requests de submit com `orchestrator_id`, sem GPU. | `gpu_device: Option<String>` + validações (§2.2). |
| `apps/web/lib/monitoring.ts` | `:24-58,70-87` | `Orchestrator.gpus`; `nodeMetrics` usa `gpus[0]` e VRAM somada. | Tipos via codegen; métricas por placa. |
| `OrchestratorCard.tsx` | `components/composite/OrchestratorCard.tsx:1-181` | Barra única de VRAM e nome da 1ª GPU. | `MultiGpuRack`. |
| `apps/web/types/studio.ts` | `:1-6` | Barrel; tipos reais em `types/api-generated.ts`. | Nada manual; codegen. |

---

## 4. Contratos

- `crates/heph-contracts/src/nodes.rs`: `GpuDeviceTelemetry { index: u32, uuid: String, name: String, vram_total: i64 /*MiB*/, vram_used: i64, power_watts: Option<f64>, gpu_utilization_pct: Option<f64>, temperature_c: Option<i32> }` — `Option`s com `#[serde(default, skip_serializing_if = "Option::is_none")]`; seguir o `rename_all` já usado no arquivo.
- `HeartbeatBody.gpu_devices: Vec<GpuDeviceTelemetry>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`); idem `OrchestratorItem` e `TelemetryResponse`.
- `DispatchRequest.gpu_device: Option<String>` (`default`, `skip_serializing_if`).
- DB: `orchestrators.gpu_devices JSONB NULL`; `jobs.gpu_device TEXT NULL`.
- OpenAPI (`packages/contracts/openapi.yaml`): schema `GpuDeviceTelemetry` (camelCase: `index, uuid, name, vramTotal, vramUsed, powerWatts?, gpuUtilizationPct?, temperatureC?`), `Orchestrator.gpuDevices`, telemetria `gpuDevices`; `gpuDevice` opcional em `YoloJobRequest`, `DiffusionJobRequest`, `DiffusionGenerateRequest`, `AutolabelJobRequest`, `AutotrackerJobRequest`; job (resposta) expõe `gpuDevice` (UUID efetivo, nullable). Contract ≡ router no mesmo commit da fatia que implementa a rota.
- Erros 400: `invalid_gpu_device`, `gpu_device_requires_orchestrator`, `unknown_gpu_device`, `insufficient_gpu_vram`.

---

## 5. Fluxos

### 5.1 Sampler
- Um único sampler no orchestrator: `nvidia-smi --query-gpu=index,uuid,name,memory.total,memory.used,power.draw,utilization.gpu,temperature.gpu --format=csv,noheader,nounits`, timeout 2s, `kill_on_drop`, cache compartilhado (~2s) lido pelo heartbeat e pelos collectors (nunca um `nvidia-smi` por consumidor).
- Colunas opcionais `[N/A]`/`[Not Supported]`/vazio → `None` sem descartar a placa; erro em index/uuid/name/memória descarta só aquela linha.
- Sem `nvidia-smi` (mock/CPU) → `gpu_devices` vazio.

### 5.2 Seleção e eleição
1. Submit (api-principal) valida formato; `gpuDevice` sem `orchestratorId` → 400.
2. Create (manager): índice → UUID via `TelemetryCache` do nó; GPU ausente → 400 `unknown_gpu_device`; `vram_total` < required da vram-table → 400 `insufficient_gpu_vram`; grava `jobs.gpu_device`.
3. Eleição: mantém 1 job/nó (decisão 1). Manual: checa a VRAM daquela GPU. Automático: menor GPU que cabe (decisão 2), grava `jobs.gpu_device`. Nó sem `gpu_devices`: fallback atual, `gpu_device` NULL.
4. `DispatchRequest.gpu_device` → executor `--gpus "device=<UUID>"` (fallback `ORCH_GPU_DEVICES`).
5. Antes do job, `maybe_preempt_daemon(target_gpu)`: derruba só se a GPU for a do daemon ou uma delas for desconhecida.
6. Requeue: manual mantida; automática re-escolhida.

### 5.3 `sys.gpu.*` por job
- O collector anexa `sys.gpu.util_pct`, `sys.gpu.temp_c`, `sys.gpu.power_w`, `sys.gpu.vram_used_mb` ao `metrics` dos reports que já carregam `epoch`/`step` (o UNIQUE `(job_id,key,epoch,step)` descartaria ticks sem step).
- Valor ausente = chave ausente, nunca 0 (PITFALLS:39).
- GPU do job = `dispatch.gpu_device`; sem ele, a única GPU de `ORCH_GPU_DEVICES` se for uma; senão não emite.
- Servido pela rota existente `GET /api/jobs/:id/metrics?keys=…`.

### 5.4 `vram_high`
- Avaliada no manager junto de `disk_high` (`services/manager/src/alerts/`), por job `running` com `gpu_device` resolvido no `TelemetryCache` do nó; job sem `gpu_device` em nó com 1 GPU → essa GPU; senão no-op. Nó sem telemetria = no-op.
- `ratio = vram_used / vram_total` da placa. `ALERT_VRAM_RATIO` (0.90) → warning; ≥0.95 → critical; critical volta a warning <0.90; resolve < ratio − `ALERT_VRAM_HYSTERESIS` (0.05). Mesma histerese e padrão de nomes de env de `disk_high` (confirmar no código).

---

## 6. Fatias

| Fatia | Dono | Branch | Conteúdo | Depende | Aceite binário |
|---|---|---|---|---|---|
| B1 | @backend | `feat/multi-gpu-sensors` | Contratos de telemetria + sampler + heartbeat + migration `orchestrators.gpu_devices` + manager cache/registry + `GET /api/orchestrators` e telemetria com `gpuDevices` + openapi | — | Teste do parser: CSV dual-GPU → 2 devices com UUID/W/%/°C; `[N/A]`/`[Not Supported]`/vazio → `None` sem descartar placa; linha com uuid inválido descartada só ela. Heartbeat com 2 GPUs persiste `gpu_devices` e o cache; `/api/orchestrators` devolve `gpuDevices` camelCase; mock/CPU → vazio. Contract ≡ router. |
| B2 | @backend | `feat/multi-gpu-selection` | Migration `jobs.gpu_device`; `gpuDevice` nos 5 submits + validações 400; índice→UUID; eleição por GPU (auto = menor que cabe); `DispatchRequest.gpu_device`; executor por UUID; `DIFFUSION_DAEMON_GPU_DEVICE` + preempção só na mesma GPU; `gpuDevice` na resposta do job | B1 | Testes: cada um dos 4 códigos 400; índice gravado como UUID; auto escolhe a menor que cabe (empate/required desconhecido cobertos); 1 job/nó preservado; requeue mantém manual e re-elege automática; args do docker com `device=<UUID>` e fallback; preempção não ocorre em GPU diferente e ocorre na mesma/desconhecida. Cita file:line do que `vram_total_gb` guarda. |
| B3 | @backend | `feat/multi-gpu-job-metrics-alerts` | `sys.gpu.*` no collector + regra `vram_high` | B2 | Teste do collector: report com step carrega as 4 chaves; valor `None` = chave ausente; sem GPU resolvível não emite. Teste com banco de `vram_high`: dispara warning ≥0.90, critical ≥0.95, volta a warning <0.90, resolve <0.85; nó sem telemetria no-op. |
| F1 | @frontend | `feat/web-multi-gpu-rack` | Codegen + `MultiGpuRack` (VRAM/%/W/°C por placa; °C <70 neutro, 70–80 `#f59e0b`, >80 `status-alert`) no `OrchestratorCard` e `/environments`; mock/CPU sem quebra | B1 | Build/lint/typecheck do web verdes; nó com 2 GPUs renderiza 2 placas; sensor ausente não exibe 0; nó mock/CPU renderiza sem erro. |
| F2 | @frontend | `feat/web-gpu-selector` | `GpuDeviceSelect` (Automático + placas do nó escolhido, VRAM livre, placa abaixo do mínimo desabilitada) em `/treino`, `/difusao`, autolabel, autotracker, geração; exibe `gpuDevice` do job | B2, F1 | Build verde; payload envia UUID da placa ou omite em Automático; placa abaixo do mínimo desabilitada; `gpuDevice` do job visível; sem nó escolhido o seletor fica em Automático. |
| I1 | @infra | `feat/infra-multi-gpu-config` | `env.gpu.example` + `env.gpu` do nó: `DIFFUSION_DAEMON_GPU_DEVICE=<UUID 3060>`, `ORCH_GPU_DEVICES` em UUID; deploy | B2 | `docker compose … config -q` ok; env do docker-04 com os UUIDs; stack sobe e o nó reporta 2 GPUs. |

Ordem: B1 → B2 → (B3, I1); F1 após B1; F2 após B2 e F1. Cada fatia fecha com veredito do `@reviewer`.

---

## 7. Aceite final (smoke real no docker-04, conduzido pelo orchestrator, logs brutos)

1. `/api/orchestrators` mostra 2 placas com UUID, VRAM, W, %, °C.
2. Job manual na 1660S (UUID) → `docker inspect` do container mostra `DeviceIDs=[GPU-c83cc056-07f7-d31e-cc98-7486ddac0296]`; `nvidia-smi` dentro do container lista só a 1660S.
3. Job automático pequeno (≤6 GB na vram-table) cai na 1660S; um que exige >6 GB cai na 3060.
4. Daemon de difusão quente na 3060 não é derrubado por job na 1660S; é derrubado por job na 3060.
5. GPU manual abaixo do mínimo → 400 `insufficient_gpu_vram`.
6. `GET /api/jobs/:id/metrics?keys=sys.gpu.util_pct,sys.gpu.temp_c` não vazio e coerente com a placa do job.
7. `vram_high` dispara/resolve com histerese (teste com banco; no smoke, ao menos não dispara falso).

---

## 8. Riscos

1. **OOM por escolha manual** em placa pequena → validação 400 `insufficient_gpu_vram` no create + placa desabilitada na UI.
2. **Spawn excessivo de `nvidia-smi`** (heartbeat + collectors) → sampler único com cache ~2s.
3. **Nós mock/CPU/legados** → campos `Option`/`Vec` com default vazio; fallback `ORCH_GPU_DEVICES`; `vram_high` no-op.
4. **Daemon na mesma GPU sem liberar VRAM a tempo** → a preempção aguarda o encerramento do container antes de seguir para o job.
5. **Sensores não suportados** (`[N/A]`/`[Not Supported]`) → colunas opcionais viram `None`; só colunas essenciais descartam a linha.
6. **Swap de índice após reboot (Pitfall D9)** → UUID canônico em DB, dispatch e env; índice só na entrada.
7. **Eleição pela soma do nó** aceitaria job que não cabe em nenhuma placa → eleição por VRAM da GPU.
8. **`sys.gpu.*` sem step** seria descartado pelo UNIQUE → anexar só a reports com `epoch`/`step`.

---

## 9. Fora de escopo

- Concorrência por GPU (mais de um job por nó).
- Script de smoke permanente (`tests/smoke_multi_gpu.sh`).
