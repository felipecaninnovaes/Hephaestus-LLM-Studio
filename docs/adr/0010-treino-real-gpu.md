# ADR-0010 — Treino real @gpu: orquestrador remoto no TrueNAS substituindo o engine mock (Fatia G)

- **Status:** **ACEITA** (usuário, 2026-09-09 — com emenda E1: alvo default = RTX 3060 device 0, pre-flight obrigatório de VRAM livre — a 3060 é intermitente, roda o modo deep do graft — e 1660S como fallback explícito). Nada implementado. Este
  documento é a especificação executável da fatia "TREINO REAL @gpu"; os deltas
  de contrato abaixo são aplicados **apenas nos commits da fatia** (openapi
  junto do código, docs de texto no `docs-sync` do fim), nunca antes.
- **Data:** 2026-09-09
- **Componentes:** `services/orchestrator` (executor docker com GPU + telemetria
  nvidia-smi), `services/manager` (env `AUTO_ADOPT_LOCAL`), `engines/trainer-yolo`
  (caminho real ultralytics completo + `Dockerfile.gpu`), `infra/compose.gpu.yaml`
  (novo, TrueNAS), `infra/` (publish S3 na LAN com bind específico), `.env` do
  dev host (`TRAINER_IMAGE`, `SEAWEED_PUBLISH`, `AUTO_ADOPT_LOCAL`).
  **Sem mudança de contrato público** (spec 0.9.0 intacta) e **sem migration**.
- **Fontes:** `IDEIA.md` §1/:14-16 (config de treino gerada pelo principal);
  `docs/backend.md` §1/:34 (direção de rede — **branch "inbound direta quando há
  IP:porta alcançável" é exatamente o caso TrueNAS**), §4/:59-61 (uma imagem por
  engine; base estável testado; `engines.yaml` como registro), §6/:76-93
  (vram-table, `nvidia-smi` 2s, 1 job = 1 GPU), §8/:104-110 (adoção por token =
  futuro; local auto-adotado), §9/:166-167 (telemetria wire `gpus[]`/
  `vramUsed`/`vramTotal` **já existem**), §10/:285-291 (`orchestrators.gpus`/
  `vram_total_gb` existem, nunca escritos pelo manager); `docs/adr/0007-jobs-v1.md`
  D5 (modo real @gpu documentado, não implementado), D2 (credencial S3 escopada
  `heph-orchestrator` + invariante de prefixo), D4 (dispatch/report/heartbeat
  HTTP), D8 (artefatos `artifacts/<job_id>/`), D9 (telemetria sem GPU = no-op
  honesto); `docs/adr/0009-web-integracao-monitoramento.md` D1 (lista de
  orquestradores; gauges só quando `items.length===1`), R1 (heartbeat sem
  identidade; cache global); `docs/dividas.md` (telemetria por orquestrador,
  adopt/rotate por token, watchdog); `docs/coordenacao.md` sessão 16 (spike
  TrueNAS — resultados abaixo); `docs/repo-estrutura.md` (ordem de fatias).
  Código (verificado por graft/grep nesta data):
  `services/orchestrator/src/lib.rs` (`DispatchRequest` L20-29 sem campo de GPU;
  `TrainerExecutor` L496-508; `DockerExecutor` L511-560 — `docker run --rm -v …
  <image> <args>` **sem `--gpus`/`--shm-size`/env**; `run_job_inner` L671-955 —
  volumes `ORCH_VOL_*` default `infra_*`; heartbeat `gpus: vec![]` fixo em
  `services/orchestrator/src/main.rs` L362-370; `scoped_key` L155-170),
  `services/manager/src/lib.rs` (`dispatch_next` L1038-1132 — **`SELECT id,
  endpoint FROM orchestrators WHERE status='online' LIMIT 1`**: escolha
  ARBITRÁRIA entre orquestradores online; `receive_heartbeat` L804-830 —
  atualiza `last_heartbeat` de TODOS os `online/degraded` e o cache global sem
  identidade; `adopt_orchestrator` L888-900 — auto-adota `orchestrator-local` no
  boot), `services/manager/src/main.rs` L386-480 (boot: adopt + recovery +
  dispatch worker; envs `TRAINER_IMAGE`/`EXEC_MODE`/`ORCH_WORKDIR` — **a imagem
  do trainer é decidida pelo MANAGER, não pelo orquestrador**),
  `engines/trainer-yolo/src/trainer_yolo/train.py` L196-227 (`_real_train` —
  `YOLO(model)` + `model.train(project=output, name="train")`: **saída em
  `output/train/weights/` e métricas em `results.csv`, não no contrato flat do
  orquestrador**; sem metrics.jsonl no caminho real), `infra/compose.yaml`
  L45-55 (SeaweedFS loopback + comentário do `SEAWEED_PUBLISH`), L97-136
  (manager/orchestrator-local 0.0.0.0), `infra/seaweedfs-s3.json` (identidades
  `heph-admin` e `heph-orchestrator` — escopo `packages/*`+`artifacts/*`),
  `packages/policies/vram-table.yaml` (yolo11n=6, yolo11m=10), `packages/
  policies/engines.yaml` (`toolchain: {cuda: TBD, torch: TBD}`).
- **Sequência:** 3e→3f→3g→4→5→F6 → **G (esta: treino real @gpu)** → dívidas
  registradas.

## FATO (spike TrueNAS, sessão 16 — NÃO re-questionar)

Ambiente provado por SSH em 2026-09-09 (coordenador via spike de ambiente):

- Host TrueNAS `dockeruser@10.15.1.2`, BatchMode OK, no grupo docker, **sem
  sudo**. Docker 28.3.1 client/server. Runtime `nvidia` (nvidia-container-toolkit
  **1.19.1**) registrado **E default** no daemon — todo container herda acesso à
  GPU por default; smoke `nvidia/cuda:12.4.1-base` + `nvidia-smi` = 2 GPUs, exit 0.
- GPUs: **RTX 3060 12GB LIVRE** (o usuário derrubou o llama.cpp que a ocupava;
   provado `0/12288 MiB` em 2026-09-09) + **GTX 1660 Super 6GB LIVRE**
   (Turing sm_75). **A 3060 é INTERMITENTE**: o usuário a usa quando roda o modo
   deep do índice graft — o pre-flight da sessão GPU (G.6/README) OBRIGA conferir
   `nvidia-smi` (VRAM da 3060 ≈ 0) antes de apontar `ORCH_GPU_DEVICES=0`; se
   ocupada, a sessão usa a 1660S (`ORCH_GPU_DEVICES=1`). Driver 580.173.02, CUDA
   13.0 no host (container cu124 roda — compatibilidade retroativa driver).
- Disco: DockerRootDir `/mnt/.ix-apps/docker` com 327G livres; `/mnt/DADOS` 4.9T.
  Build de imagem PyTorch (~8-10GB) viável NO HOST.
- Repo clonado em `/mnt/DADOS/home/dockeruser/Hephaestus-LLM-Studio`, HEAD =
  main `1a477dd`, remote Gitea `ssh://git@git.felipecncloud.com:2222/…`, chave
  `id_git_ed25519` presente → **o TrueNAS recebe o código por `git pull` da main**.
- Rede: TrueNAS alcança `10.15.10.3` (host dev) em 8080/8081/8082/3000 (0.0.0.0 no
  compose). S3 SeaweedFS **loopback-only** (`127.0.0.1:8333`, R1 ADR-0003 —
  "LAN nunca toca o bucket"; compose.md L49-54 documenta como publicar).
  Postgres 5432 alcançável mas **NUNCA expor ao orquestrador remoto** (só o
  manager toca o banco).
- 48 containers no host TrueNAS (gitea + gitea-runner = **CI do usuário roda
  nessa caixa**, frigate, llama-cpp, traefik…): não derrubar/portar nada; testes
  só em portas altas efêmeras.
- Teste @gpu é **MANUAL, fora do CI**.

## Contexto

O pipeline v1 (ADR-0007) entrega yolo_train local end-to-end **com engine mock**
(CPU, `best.pt` fake de 110 bytes "HEPHMOCK", métricas sintéticas). O modo real
`@gpu` do trainer-yolo existe documentado (README) e parcialmente implementado
(`_real_train`), mas: (a) nunca rodou em GPU; (b) a saída dele **não respeita o
contrato flat** do orquestrador (`output/train/weights/*` vs `outputs/<job_id>/`
+ `metrics.jsonl`); (c) o orquestrador não sabe passar GPU ao docker run; (d) a
telemetria é `gpus:[]` fixo; (e) o dispatch do manager escolhe orquestrador com
`LIMIT 1` arbitrário. O usuário ofereceu a máquina TrueNAS via SSH para o treino
real. Esta ADR desenha a fatia que liga o pipeline de jobs **sem tocar no
contrato público**: mesmo binário do orquestrador em container no TrueNAS,
registrado no manager do dev host, treinando com ultralytics real na RTX 3060 (fallback: 1660S).

Regras da casa aplicadas: mock continua default (`ENGINE_MOCK=1` no compose);
GPU real só `@gpu` manual fora do compose local; wire camelCase em `/api/*`,
transporte interno snake_case (ADR-0002 D1); erros `{code,message}` estáticos;
PRs < ~400 linhas de produção; branch `feat/treino-real-gpu`; teste @gpu manual
fora do CI.

## Decisões já travadas (base — citar, não redecidir)

- **`orchestrators` (0006) é tabela do manager; orquestrador nunca toca
  Postgres; principal só fala com o manager via Bearer `MANAGER_TOKEN`** (ADR-0007
  D3/D8). O orquestrador remoto é stateless: workspace + volumes, todo estado
  volta via report (backend.md §1/:29, §8/:111).
- **Credencial S3 do orquestrador = identidade `heph-orchestrator`** escopada a
  `packages/*`+`artifacts/*` (ADR-0007 D2, spike F4.0 provou path-scope) + **2ª
  barreira: `scoped_key` no código** (recusa qualquer key fora do prefixo).
  Package = zip autossuficiente em `packages/<version_id>/` (D1) — o orquestrador
  remoto não precisa dos volumes de dataset do dev host.
- **Dispatch/report/heartbeat = HTTP** (ADR-0007 D4); `dispatch_next` marca
  `dispatched` e o orquestrador responde; report é PUSH; heartbeat ~2s alimenta
  o cache global de telemetria do manager (D9; ADR-0009 R1: **sem identidade no
  heartbeat** — atualiza `last_heartbeat` de todos os `online/degraded`).
- **Telemetria wire `{measured, cpu, ram, ramTotal, vramUsed, vramTotal, gpus[],
  jobsActive}` já existe** (ADR-0009 D4) — GPU real só preenche campos que já
  estão no contrato; zero delta OpenAPI.
- **`GET /api/orchestrators`** lista a tabela (ADR-0009 D1); a UI só compõe
  gauges quando `items.length===1`.
- **vram-table/policy VRAM é no-op na v1** (ADR-0007 D9): `vram_min_gb` gravado
  mas não bloqueante; sem `waiting_vram`.
- **`orchestrators.gpus`/`vram_total_gb` existem no schema mas nunca são
  escritos pelo manager** (ADR-0009 D1; coluna vazia ≠ dado).
- **Ciclo e fila**: `queued→dispatched→preparing→running→done|failed|cancelled`;
  recovery no boot (`queue_reason='recovered'`); auto-adoção do
  `orchestrator-local` no boot do manager (dedupe por endpoint UNIQUE).
- **Adoção remota por token (pairing `heph_p_*`/`heph_o_*`) é FUTURO** (backend.md
  §8; ADR-0009 D0 — POST adopt/rotate/revoke pendente, dívida registrada).
- **Imagem por engine** (backend.md §4/:60): `hephaestus/trainer-yolo:local`
  (mock, stdlib pura); base "estável mais recente testado", versão validada em
  `engines.yaml` (hoje `toolchain: TBD`).

## Decisões

### D0 — Escopo da fatia G (entra/sai)

**Decidido — entra na fatia:**
- Caminho real do `trainer-yolo` **completo** (metrics.jsonl incremental com as
  6 keys, pesos flat, device/seed) + **`Dockerfile.gpu`** (nova imagem
  `hephaestus/trainer-yolo:gpu`).
- Orquestrador: passar GPU ao `docker run` (env `ORCH_GPU_DEVICES`), telemetria
  real via `nvidia-smi` (com fallback silencioso), guarda anti-mock.
- Manager: env `AUTO_ADOPT_LOCAL=0` (não ressuscitar o orquestrador local em
  sessão GPU).
- Infra: **`infra/compose.gpu.yaml`** (TrueNAS) + publish do S3 na LAN com bind
  específico + `infra/README-gpu.md` (checklist da sessão GPU) + `env.gpu.example`.
- Smoke **manual** @gpu no TrueNAS (passo final de verificação, fora do CI).

**Decidido — fora (dívida/futuro, NÃO agora):**
- **Roteamento por capacidade** no manager (substituir o `LIMIT 1` por seleção
  por GPU/VRAM/kind) — a sessão GPU usa contrato operacional (D2).
- **Heartbeat com identidade + cache por orquestrador + watchdog** (ADR-0009 R1,
  dívida registrada) — a sessão GPU evita o problema parando o orquestrador local.
- **Adoção remota por token / POST adopt/rotate/revoke** (dívida existente) — o
  v1 da sessão GPU usa INSERT manual (D2).
- **Transporte chunked** (ADR-0003 D9) — LAN é rápida; zip inteiro sobre a LAN
  continua sendo o transporte (residual R6 do ADR-0007 mantido).
- **Multi-GPU/DDP, pause/resume/checkpoint, samples** — intactos (fora).
- **Policy VRAM aplicada** (fila `waiting_vram` por orquestrador) — segue no-op;
  só documentação de envelope seguro (D9).
- **Mudança de contrato público** — não há (D11).

**Descartado:** "subir o stack inteiro no TrueNAS" (Postgres/manager são do dev
host; TrueNAS jamais toca o banco — FATO spike); "o orquestrador remoto baixa o
peso do modelo via HF a cada job" (rede externa + não-reprodutível — bake no
build, D5/D6); "expirar o local para `offline` por watchdog antes da fatia"
(watchdog é dívida; sessão GPU não depende dele — D2).

### D1 — Topologia: MESMO binário do orquestrador, instância dedicada no TrueNAS

**Decidido:** o orquestrador remoto é uma **nova instância do binário atual**
(`services/orchestrator`, sem mudança de processo), em container no TrueNAS via
`infra/compose.gpu.yaml` (novo, nome do projeto `gpu`), registrada no manager do
dev host por **INSERT manual na tabela `orchestrators`** (ops, 1 psql) —
`kind='remoto'`, `endpoint='http://10.15.1.2:8082'`, `status='online'`, e
preenchendo **`gpus` JSONB + `vram_total_gb=18`** (colunas existentes — dado
estático real do spike; isso diverge da nota "nunca escritos no v1" do ADR-0009
D1 — ver "O que fica falso"). O heartbeat existente (~2s, Bearer `MANAGER_TOKEN`)
mantém a linha `online` e o cache de telemetria sem nenhuma mudança de contrato.
Dispatch/report/artifacts fluem pelo caminho HTTP existente:
manager (dev) → `http://10.15.1.2:8082` (LAN, branch "inbound direta quando há
IP:porta alcançável" de backend.md §1/:34); orquestrador → manager
`10.15.10.3:8081`; S3 → `10.15.10.3:8333` (D3).

*Por quê — reuso total:* o pipeline `run_job_inner` (download→md5→unzip→config→
docker run→metrics→upload→report), o `scoped_key`, a idempotência de dispatch e
o recovery do manager são agnósticos de onde o orquestrador roda; zero mudança
no contrato manager↔orquestrador. O orquestrador remoto é **stateless por
desenho** (backend.md §1/:29) — só precisa de S3 + manager URL + volumes locais.
*Gotcha:* a imagem do trainer é decidida pelo **manager** (`TRAINER_IMAGE`), não
pelo orquestrador — o remoto precisa que a imagem exista no daemon local dele
(build no TrueNAS, D5) e a guarda anti-mock cobre o caso de imagem errada (D2/D4).
*Descartado:* container do orquestrador remoto rodando no dev host com
`DOCKER_HOST` apontando pro TrueNAS (introduz dependência de TLS/socket remoto e
paths de volume cruzados; o binário em container no próprio TrueNAS é o caminho
já provado pelo spike — socket local, volumes locais).

### D2 — Seleção de orquestrador: contrato operacional de "sessão GPU" (LIMIT 1 arbitrário mitigado)

**FATO (código):** `dispatch_next` faz `SELECT id, endpoint FROM orchestrators
WHERE status = 'online' LIMIT 1` — sem `ORDER BY`, escolha **arbitrária** quando
há 2+ orquestradores online; e **não há watchdog** (status nunca sai de `online`
— dívida F4.8). Portanto, "parar o container do orquestrador local" **não basta**
para rotear para o remoto.

**Decidido — sessão GPU = contrato operacional com 3 passos (checklist em
`infra/README-gpu.md`, G.4):**
1. `docker compose stop orchestrator-local` (para o heartbeat do local — ver D8);
2. `DELETE FROM orchestrators WHERE kind='local'` + INSERT da linha remota (D1);
3. `AUTO_ADOPT_LOCAL=0` no `.env` do dev host + recreate do manager — senão o
   próximo boot do manager **ressuscita o local** e o próximo dispatch pode
   cair no mock (trap silencioso).

Com a linha local removida, o remoto é o único `online` → todo dispatch vai para
o TrueNAS. Fim da sessão: teardown inverso (G.6/README).

**Decidido — guarda anti-mock no orquestrador remoto (5-15 linhas):** se
`ORCH_GPU_DEVICES` estiver setado (remoto em modo GPU) e `dispatch.image`
contiver `hephaestus/trainer-yolo:local`, o dispatch é recusado com erro claro
(`409 invalid_request`? **não** — erro interno do pipeline: `Err` no dispatch
com mensagem "GPU orchestrator requires GPU trainer image (TRAINER_IMAGE=…
:gpu)"). Escapada explícita `ORCH_GPU_ALLOW_MOCK=1` para testes. *Por quê:* o
trap mais perigoso da fatia é o **treino fake silencioso** (job `done` com
`best.pt` de 110 bytes quando o operador esqueceu `TRAINER_IMAGE`); a guarda
transforma o erro em **falha visível**, e o critério de aceite do smoke também
confere o tamanho do artefato (D12/G.6).

*Gotcha:* a imagem de fato (`:gpu` vs `:local`) é config no dev host
(`TRAINER_IMAGE` no `.env` + recreate do manager) — a guarda só pega o caso mais
comum (imagem default); o checklist do README manda conferir `docker compose
config` e o tamanho do artefato. *Dívida registrada:* roteamento por capacidade
(seleção por GPU/VRAM/kind + `vram_min_gb` vs `orchestrators.vram_total_gb`) é a
substituição correta do LIMIT 1 e do contrato manual — fatia futura.
*Descartado:* `ORDER BY (kind='remoto') DESC` no dispatch (rotearia TODOS os jobs
para o remoto sempre que online, inclusive os que não precisam de GPU — mudança
de semântica global sem consumidor); aceitar o LIMIT 1 arbitrário sem mitigação
(trap silencioso de mock em produção de uso real).

### D3 — Rede S3: publicar o SeaweedFS na LAN com bind no IP específico do dev host

**FATO (código/spike):** SeaweedFS publicado em `127.0.0.1:8333` (compose L55,
`${SEAWEED_PUBLISH:-127.0.0.1}`); R1 ADR-0003 = "a LAN nunca toca o bucket";
credenciais LOCAL-DEV (`heph`/`heph-orch`); o orquestrador remoto **precisa** de
S3 para baixar o package e subir artifacts (imutável — pipeline D4 ADR-0007).

**Decidido (opção a refinada):** publicar o S3 **apenas na interface de LAN do
dev host** — `.env`: `SEAWEED_PUBLISH=10.15.10.3` (docker bind
`10.15.10.3:8333:8333`), **não** `0.0.0.0`. O orquestrador remoto usa
`S3_ORCH_ENDPOINT_URL=http://10.15.10.3:8333` (client SigV4 direto, sem
presigned). `S3_PUBLIC_ENDPOINT_URL` do principal **permanece**
`http://localhost:8333` (presigned assinado nesse host — gotcha SigV4 do
ADR-0003 D3; o browser não muda). SeaweedFS interno continua `-ip.bind=0.0.0.0`
(é o container; quem decide é o publish).

*Por quê — avaliando R1 conscientemente:* LAN caseira, credencial do
orquestrador escopada a `packages/*`+`artifacts/*` (spike F4.0 provou path-scope)
+ invariante de prefixo no código (2ª barreira), e o **bind em IP específico**
reduz a superfície (não é `0.0.0.0`); o próprio compose antecipa o mecanismo
(`SEAWEED_PUBLISH` + comentário L49-54). Nenhum caminho de dados novo: o
orquestrador remoto **não** ganha acesso ao Postgres nem ao master 9333 (não
publicado). *Gotcha:* credenciais `heph-admin` continuam LOCAL-DEV e agora são
alcançáveis da LAN — mitigação = bind específico + confiança da LAN caseira
(estado atual de 8080/8081/8082/3000 já é 0.0.0.0; **não pioramos**) + dívida:
rotacionar credenciais e exigir TLS se o ambiente sair da LAN.
*Descartado:* (b) **túnel SSH reverso** (`ssh -R 8333:127.0.0.1:8333` dev→TrueNAS)
— mantém loopback mas adiciona peça móvel persistente (autossh, queda = job
falha não-óbvia) sem ganho de segurança real numa LAN caseira já confiada;
(c) **proxy de bytes via manager/principal** — viola o boundary (manager não tem
S3; principal é BFF do front, não gateway de orquestrador) e exige rotas internas
novas de upload que não existem; descartado por arquitetura, não por conveniência.

### D4 — GPU: seleção por instância via `ORCH_GPU_DEVICES`; default 3060 (device 0)

**FATO (spike):** runtime `nvidia` é **default** no daemon TrueNAS — todo
container ganha acesso à GPU (e `nvidia-smi` injetado pelo toolkit); sem pin,
o trainer veria **as duas** GPUs (e a 3060 ocupada).

**Decidido:** o orquestrador lê **`ORCH_GPU_DEVICES`** (default ausente = caminho
mock/local intocado; valor = lista de índices nvidia-smi, ex. `"1"`) e o
`DockerExecutor` passa ao `docker run` do trainer:
- `--gpus "device=1"` (portável, funciona com ou sem runtime default) **e**
  `-e NVIDIA_VISIBLE_DEVICES=1` (reforço para daemons com runtime nvidia
  default — belt and suspenders, custo zero);
- `--shm-size=2g` (DataLoader do ultralytics usa shm; 64MB default é causa
  clássica de OOM esquisito em treino de verdade) — aplicado **sempre que**
  `ORCH_GPU_DEVICES` estiver setado;
- `-e ENGINE_MOCK=0` (reforço; o default real vem baked na imagem gpu, D5).

O **compose.gpu.yaml** fixa `ORCH_GPU_DEVICES=0` (a 3060, default da fatia) — decisão por
instância, não por job (1 orquestrador = 1 dispositivo de treino). O pre-flight da sessão
confere a VRAM da 3060; se ocupada (graft deep), o operador sobe o remoto com
`ORCH_GPU_DEVICES=1` (1660S) — o env é o único ponto de mudança, nenhum código. *Por quê:* o dispatcher (D4 ADR-0007) não carrega campo
de GPU e não deve carregar nesta fatia (sem delta de contrato); a configuração
por instância é o desenho §6/:93 ("1 job = 1 GPU, `CUDA_VISIBLE_DEVICES` escolhido
pelo orquestrador") — aqui o orquestrador inteiro é dedicado à 1660S.
*Gotcha:* `--gpus "device=1"` refere-se ao **índice nvidia-smi do host** — se a
ordem das GPUs mudar (reboot com dispositivos diferentes), o índice pode mudar;
o smoke G.6 valida `nvidia-smi -L` no host antes da sessão e o README documenta.
*Descartado:* `--gpus all` + `CUDA_VISIBLE_DEVICES` só dentro do trainer (o
container ainda carregaria libs de 2 GPUs; pin no runtime é mais determinístico);
configurar GPU por job no dispatch (contrato novo sem necessidade — instância
dedicada).

### D5 — Imagem do trainer GPU: `Dockerfile.gpu` separado, build no TrueNAS, peso baked

**Decidido — DUAS imagens, não uma com switch:**
- `hephaestus/trainer-yolo:local` — **intocada** (mock stdlib puro, ~200MB,
  CI local rápido; é o caminho verificado do compose do dev host).
- **`hephaestus/trainer-yolo:gpu`** — nova `engines/trainer-yolo/Dockerfile.gpu`:
  base `pytorch/pytorch:2.6.0-cuda12.4-cudnn9-runtime` (digest pinado na
  implementação; sm_75/Turing suportado pelo cu124; driver 580 do host é
  retrocompatível com runtime cu124 — provado no spike com `cuda:12.4.1-base`),
  `pip install "ultralytics==8.3.x"` (pin exato no Dockerfile; registrar em
  `engines.yaml` na sync G.7), `COPY src/trainer_yolo/`, **`ENV ENGINE_MOCK=0`**,
  e **`RUN python -c "from ultralytics import YOLO; YOLO('yolo11n.pt')"`** no
  build — baixa e cacheia o peso base **na camada da imagem** (~5MB; sem
  dependência de rede no runtime para o modelo default; reprodutível).

*Por quê:* (a) manter UMA imagem com switch inflaria o mock em ~8GB e deixaria o
CI local lento — a lição "mock é stdlib pura" (ADR-0007 D5) vale; (b) base
ultralytics oficial (`ultralytics/ultralytics:8.3.x`) rejeitada: menos controle
de pin, imagem maior, e o nosso `train.py` já isola o caminho real; (c) build
**no TrueNAS a partir do repo clonado** (`docker compose -f infra/compose.gpu.yaml
--profile build build trainer-gpu`) — FATO spike: 327G livres no DockerRootDir,
rede OK, sem scp de imagem. A imagem gpu **não é construída** no CI/dev host
(perfil build ausente do compose local; o `:local` continua o único do dev).
*Gotcha:* modelos além de `yolo11n` (yolo11m/11x/yolov9-c/11-seg) baixam o peso
no **runtime** (rede do TrueNAS — OK hoje; em ambiente offline, bake adicional no
Dockerfile.gpu). *Descartado:* imagem única com `ENGINE_MOCK` por env (infla o
mock e quebra o "mock não instala torch"); scp/rsync de imagem do dev (desnecessário
com build no host — FATO spike).

### D6 — Código do trainer-yolo: caminho real completo (contrato flat + metrics incremental)

**FATO (código):** `_real_train` hoje faz `model.train(project=output,
name="train")` → pesos em `output/train/weights/best.pt|last.pt` e métricas em
`output/train/results.csv`; o orquestrador espera **`outputs/<job_id>/best.pt`**,
`last.pt` e **`metrics.jsonl`** (6 keys: `epoch, box_loss, cls_loss, dfl_loss,
mAP50, mAP50-95`) lido incrementalmente pelo coletor (~2s). Sem ajuste, um treino
real: (a) não produz artefatos (o upload não acha os arquivos flat → job done sem
artefatos) e (b) não reporta progresso (metrics.jsonl nunca existe até o fim).

**Decidido — completar `_real_train`:**
1. `device=0` (dentro do container com `NVIDIA_VISIBLE_DEVICES=1`, o device 0 é
   a 1660S mapeada) e `seed=` do config (paridade com o mock);
2. **callback `on_train_epoch_end`** que append a linha do `metrics.jsonl` (6
   keys exatas, conversão das colunas `train/box_loss`, `train/cls_loss`,
   `train/dfl_loss`, `metrics/mAP50(B)`, `metrics/mAP50-95(B)` do `trainer.metrics`
   do ultralytics) — o coletor do orquestrador passa a reportar progresso real
   por época sem mudança no Rust;
3. pós-train: `shutil.copy` de `output/train/weights/{best,last}.pt` →
   `output/` flat + `output/metrics.jsonl` fechado com a última linha (o upload
   de artefatos do orquestrador continua idêntico);
4. `project=output, name="train", exist_ok=True` mantido; `mosaic=float(...)` e
   `mixup=0.5|0.0` já corretos (L221-223).

O **mock continua default e intocado** (`ENGINE_MOCK=1`); o caminho real só roda
na imagem gpu (`ENGINE_MOCK=0` baked) ou manualmente com extras `[train]`.
Testes: pytest com ultralytics **mockado** (fixture de `trainer.metrics` +
`results.csv` sintético) cobrindo o callback (6 keys, incremento), o copy flat e
o parse das colunas; CI local não roda GPU (dívida existente do pytest no CI —
não ampliada).

*Por quê:* o contrato de artefatos/metrics do ADR-0007 D5/D8 é o que a UI, o
manager e o `GET /api/jobs/:id/metrics` consomem — o caminho real precisa
**aderir ao contrato**, não criar um segundo formato. *Gotcha:* ultralytics muda
nomes de coluna entre minor versions — o pin `8.3.x` no Dockerfile.gpu + o teste
de parse fixam o contrato. *Descartado:* mudar o orquestrador para ler
`output/train/weights/` (contrato de artefatos é público — `path` relativo a
`artifacts/<job_id>/`; quebraria o job_artifacts e o download da UI);
treinar sem callback e só fechar o jsonl no fim (job real de 30+ min ficaria sem
progresso — o painel /jobs e o abort perdem a graça e o coletor de métricas fica
morto).

### D7 — Código do orquestrador: `TrainerExecutor` com GPU/env + telemetria nvidia-smi

**Decidido — executor:** a trait `TrainerExecutor::run` ganha **parâmetros novos
(env e gpu_devices)** — assinatura `run(image, container_name, volumes, args,
env: &[(String,String)], gpu_devices: Option<&str>)` (ou variante com struct de
opções; decisão de implementação). `DockerExecutor`:
- `gpu_devices Some(v)` → `--gpus "device={v}"` + `-e NVIDIA_VISIBLE_DEVICES={v}`
  + `--shm-size=2g` + envs repassados;
- `None` → comportamento atual byte-a-byte (mock local intocado).
`run_job_inner` lê `ORCH_GPU_DEVICES` (uma vez, boot) e monta `env=[("ENGINE_MOCK","0")]`
quando setado. `SubprocessExecutor` ignora os novos parâmetros (stub honesto
inalterado — RunPod é fatia futura).

**Decidido — telemetria real com fallback silencioso:** no loop do heartbeat
(~2s), tentar `nvidia-smi --query-gpu=name,memory.total,memory.used
--format=csv,noheader,nounits` (o toolkit injeta `nvidia-smi` no container do
orquestrador quando ele tem acesso à GPU — ver D8). Sucesso → `gpus[]` com os
nomes reais e `vram_total`/`vram_used` = **soma sobre as GPUs visíveis** (D8).
Falha (binário ausente — orquestrador local sem GPU) → comportamento atual
(`gpus: vec![], vram_*: None`) **sem log de erro spam** (um warn no boot). Teste:
função pura de parse com fixture do CSV do nvidia-smi (3 linhas, 2 GPUs).

*Por quê:* a telemetria é o único caminho que a UI já tem para GPU (ADR-0009 D4 —
`gpus[]` + gauges); preencher com dado real é o "bom caso de teste pro gauge"
registrado no spike. *Gotcha:* `memory.used` do nvidia-smi é **global por GPU**
(inclui processos do host/outros containers — exatamente o que queremos mostrar:
a 3060 ocupada aparece como ocupada). *Descartado:* reportar por-GPU no wire
(contrato tem um par `vramUsed/vramTotal` — mudança de shape seria delta
público desnecessário); `torch.cuda` no orquestrador (orquestrador é Rust;
nvidia-smi é a fonte §6/:75).

### D8 — Telemetria e visibilidade de GPU do orquestrador remoto

**Decidido:** no TrueNAS, o container **do orquestrador** vê **as 2 GPUs**
(`gpus: all` explícito no compose.gpu.yaml — embora o runtime nvidia default já
dê; explícito documenta intenção) → heartbeat reporta os nomes reais
(`gpus:["NVIDIA GeForce RTX 3060","NVIDIA GeForce GTX 1660 SUPER"]`) e
`vram_total`=soma (~18GB), `vram_used`=soma (~11.5GB → gauge "sem GPU (mock)" vira
número real no dashboard). O **trainer** vê só a 1660S (`NVIDIA_VISIBLE_DEVICES=1`,
D4). Com o orquestrador local **parado** (D2 passo 1), o cache global de
telemetria não oscila entre dois nós — **crítico**: `receive_heartbeat` não tem
identidade (ADR-0009 R1) e sobrescreve o cache a cada ~2s; se o local continuar
heartbeating, a telemetria alterna entre CPU/RAM do dev host e do TrueNAS (dado
mentiroso por oscilação). O passo "stop do local" é o que torna a sessão GPU
honesta também na telemetria — e, de brinde, com `items.length===1` o dashboard
volta a compor os gauges (regra ADR-0009 D1).

*Gotcha:* `jobs_active` reportado pelo remoto é o dele (correto — é quem executa).
*Dívida:* heartbeat com identidade + cache por nó (ADR-0009 R1) continua sendo o
conserto de longo prazo para 2+ orquestradores simultâneos.

### D9 — VRAM (12GB na 3060 / 6GB na 1660S) e a policy: vram-table continua no-op; envelope seguro documentado

**FATO (vram-table):** `yolo11n train = 6GB`, `yolo11m train = 10GB`, headroom 2.
A 3060 tem 12GB físicos; a 1660S, 6GB.

**Decidido:** a policy **permanece no-op na v1** (sem `waiting_vram`, sem
bloqueio — ADR-0007 D9): `vram_min_gb` continua gravado mas não bloqueante. A
fatia documenta o **envelope seguro** (README-gpu + nota no sync): na **3060
(default)**, `yolo11n` com `batch=16` e `yolo11m` com `batch=8` (≈10-11GB — a
entrada `yolo11m=10` + headroom 2 fecha em 12GB, no limite) são o envelope;
`yolo11x`/`batch≥32` tendem a OOM. Na **1660S (fallback)**, só `yolo11n` com
`batch=8` e `imgsz=640` (≈4-5GB) → **falha honesta e visível** (job `failed` com log do ultralytics —
o pipeline reporta o erro; não é silencioso). *Por quê:* implementar a policy
aplicada (fila `waiting_vram` por orquestrador + `vram_min` vs `vram_total`) é a
fatia de roteamento por capacidade (D2 — dívida); o no-op + documentação é o
escopo certo para ligar o treino real sem reescrever o dispatcher.
*Nota registrada:* a entrada `yolo11n=6` com headroom 2 **não cabe** nos 6GB
da 1660S — a fatia de policy terá de revisar entradas/headroom (na 3060,
`yolo11m=10` + headroom 2 fecha exato em 12GB: apertado, validar no smoke).
*Descartado:* bloquear `vram_min_gb > 6` no submit (policy de verdade sem
consumidor de VRAM por orquestrador = decisão arbitrária; e o mock local
continuaria aceitando qualquer coisa — inconsistente).

### D10 — Segurança: aceites conscientes (LAN caseira) + o que NÃO piora

**Decidido (aceites conscientes, registrados como dívida onde couber):**
1. **Registro remoto sem token** (INSERT manual + auto-adoção existente): o
   manager despacha para QUALQUER `online` — um host LAN malicioso com o
   `MANAGER_TOKEN`/endpoint correto receberia jobs. Aceito: LAN caseira, mesmo
   postura do auto-adopt local por rede docker (ADR-0007 D3); dívida já
   registrada (adopt/rotate/revoke por token — ADR-0009 D0). O `MANAGER_TOKEN`
   do remoto é o MESMO do dev host (secret compartilhado; rotacionar = dívida
   futura da gestão de orquestradores).
2. **S3 na LAN** (D3): bind em IP específico + identidade escopada
   `heph-orchestrator` + invariante de prefixo (2ª barreira) — aceito; dívida:
   rotacionar credenciais LOCAL-DEV e exigir TLS se o ambiente sair da LAN
   (postura já existente do compose L49-54).
3. **Manager/principal/web já 0.0.0.0 na LAN** — estado atual, **não pioramos**
   (nenhum bind novo além do S3 em IP específico).
4. **Postgres NUNCA ao remoto** — respeitado (FATO spike); o orquestrador
   remoto não recebe `DATABASE_URL` no compose.gpu.yaml.

**Decidido — secrets no remoto:** `env.gpu` no TrueNAS com `MANAGER_TOKEN`,
`S3_ORCH_ACCESS_KEY`/`S3_ORCH_SECRET_KEY` (a MESMA `heph-orchestrator` — escopo
`packages/*`+`artifacts/*` já cobre o remoto; nenhuma credencial nova) e
`S3_ORCH_ENDPOINT_URL=http://10.15.10.3:8333`. Nada de `POSTGRES_*`/credenciais
admin no arquivo.

### D11 — Contrato, schema e versionamento: ZERO delta público

**Decidido:** **nenhuma rota pública nova, nenhum `Error.code` novo, nenhum
campo novo no wire, spec 0.9.0 intacta.** Os campos de telemetria que passam a
ter valor real (`gpus[]`, `vramUsed`, `vramTotal`) **já existem no contrato**
(ADR-0009 D4). Rotas internas manager↔orquestrador e o `DispatchRequest` **não
mudam** (GPU é config de instância, não de job — D4). **Migration: nenhuma**
(`orchestrators.gpus`/`vram_total_gb` já existem — D1 só os preenche). **Spike
obrigatório? NÃO** — o spike de ambiente já rodou (FATOs acima); as premissas
técnicas restantes (injeção de `nvidia-smi` pelo toolkit, cu124 × sm_75,
ultralytics 8.3.x × torch 2.6) são comportamento padrão da stack, verificadas
baratas no **pre-flight do smoke G.6** (D12). Se o pre-flight falhar:
- `nvidia-smi` ausente no container do orquestrador → mount read-only de
  `/usr/bin/nvidia-smi` do host no compose.gpu.yaml (fallback, zero código);
- peso `yolo11n.pt` não baixa no build (rede) → bake via volume `models/` no
  primeiro run (documentado; rede já provada OK no spike).

**Env novos (lista de contrato interno — delta):**
| Serviço | Env | Default | Efeito |
|---|---|---|---|
| orchestrator | `ORCH_GPU_DEVICES` | ausente | setado → `--gpus device=…` + `--shm-size=2g` + `-e ENGINE_MOCK=0` no docker run; ausente → comportamento atual |
| orchestrator | `ORCH_GPU_ALLOW_MOCK` | ausente | escapa a guarda anti-mock (D2) |
| orchestrator | (telemetria) | — | tenta `nvidia-smi`; falha → `gpus:[]` atual |
| manager | `AUTO_ADOPT_LOCAL` | `1` | `0` → não auto-adota `orchestrator-local` no boot (D2) |
| dev host .env | `TRAINER_IMAGE` | `hephaestus/trainer-yolo:local` | sessão GPU: `hephaestus/trainer-yolo:gpu` + recreate manager |
| dev host .env | `SEAWEED_PUBLISH` | `127.0.0.1` | sessão GPU: `10.15.10.3` (D3) + `--force-recreate` do seaweedfs (lição F4.6 bug #5: bind-mount não detecta mudança de env) |
| TrueNAS env.gpu | `MANAGER_URL`/`S3_ORCH_*`/`ORCH_GPU_DEVICES` | — | apontam para `10.15.10.3` (D1/D3/D4) |

## Testes

| Camada | Infra | Prova |
|---|---|---|
| `pytest engines/trainer-yolo` | unit (ultralytics mockado) | callback `on_train_epoch_end` grava linha metrics.jsonl com as 6 keys exatas por época (fixture de `trainer.metrics`); conversão `mAP50(B)`→`mAP50`, `mAP50-95(B)`→`mAP50-95`; copy flat de `train/weights/{best,last}.pt` → `output/`; parse tolerante de resultados ausentes (época sem métrica não quebra) |
| `cargo test -p orchestrator` | unit (sem S3/GPU) | parse do CSV do nvidia-smi (fixture com 2 GPUs → nomes + soma de vram; linhas malformadas → fallback); `run` do DockerExecutor monta `--gpus device=1`/`--shm-size=2g`/`-e` só com `Some` (e NÃO monta com `None` — mock intocado); guarda anti-mock recusa `image` `:local` com `ORCH_GPU_DEVICES` setado e passa com `:gpu`/escapada |
| `cargo test -p manager -- --ignored` + `scripts/test-db.sh` | Postgres do compose | (G.3) `AUTO_ADOPT_LOCAL=0` → boot **não** insere `orchestrator-local` (linha ausente após boot); `=1`/ausente → comportamento atual (1 row) |
| Smoke manual @gpu (**fora do CI**, G.6, via SSH) | TrueNAS + dev host | critérios binários em D12 |

O CI cobre só as baterias locais (pytest + cargo dos 3 + test-db + contract —
spec 0.9.0 intocada, contract continua verde); o smoke @gpu é **manual** (FATO
spike) e não entra em job de CI.

## Spike obrigatório? — NÃO (spike de ambiente já executado; pre-flight no smoke)

Os FATOs do ambiente (runtime nvidia default, 1660S livre, disco, rede, repo
clonado, 48 containers intocáveis, Postgres fora) vieram do spike da sessão 16.
As 3 premissas técnicas restantes são comportamento padrão (toolkit injeta
`nvidia-smi`; cu124 roda em sm_75 com driver 580; ultralytics 8.3.x instala em
torch 2.6 cu124) e **não justificam spike separado**: entram como pre-flight do
passo manual G.6, com fallback explícito em D11. O que inverteria o desenho (e
aí sim vira spike): `nvidia-smi` não injetável e sem mount viável → telemetria
fica `gpus:[]` e a sessão GPU perde o gauge (não bloqueia o treino; reavaliar
fonte de telemetria); runtime `--gpus` rejeitado pelo daemon TrueNAS (não
provável — `--runtime=nvidia` já smokeado) → fallback só `NVIDIA_VISIBLE_DEVICES`
+ `--runtime=nvidia` explícito.

## Riscos e contingências

- **R1 — LIMIT 1 arbitrário / trap do mock silencioso:** com 2 orquestradores
  online, o dispatch pode cair no local (mock) e o job "concluir" com peso fake.
  Mitigação em camadas: contrato de sessão (stop local + DELETE row +
  `AUTO_ADOPT_LOCAL=0`, D2) + guarda anti-mock no remoto + critério de aceite
  confere `orchestrators` (1 row `remoto`) e **tamanho do artefato** (G.6).
  Residual: operador que pular o checklist → dívida registrada (roteamento por
  capacidade).
- **R2 — Cache de telemetria oscilando entre nós** (heartbeat sem identidade,
  ADR-0009 R1): stop do orquestrador local é parte OBRIGATÓRIA da sessão (D8);
  sem ele, a telemetria alterna CPU/RAM entre dev e TrueNAS a cada ~2s.
- **R3 — OOM no treino real** (12GB na 3060 / 6GB na 1660S): envelope seguro (D9); OOM vira job `failed`
  com log visível (não silencioso). `--shm-size=2g` cobre o OOM clássico do
  DataLoader.
- **R4 — S3 na LAN:** superfície reduzida por bind em IP específico + identidade
  escopada + invariante de prefixo; dívida de rotação/TLS fora da LAN.
- **R5 — `dataset.yaml` do package × ultralytics:** paths relativos materializados
  pelo builder podem precisar de ajuste (`path:`/`nc:` já presentes no package);
  validado no smoke G.6 (o treino real de 3 épocas falha rápido se o yaml não
  resolver — erro visível, não bloqueia o desenho).
- **R6 — Índice de GPU instável entre reboots** (`device=1`): pre-flight
  `nvidia-smi -L` no host + README documenta conferir antes da sessão.
- **R7 — Peso de modelo não-default no runtime** (yolo11m/…): rede do TrueNAS OK;
  bake adicional documentado para offline.
- **R8 — Image build pesado no TrueNAS** (~8-10GB, 5-15 min): disco provado
  (327G); é build-only (`--profile build`), não afeta os 48 containers.
- **R9 — test-db.sh varre `orchestrators`** (dívida conhecida): rodar test-db
  durante uma sessão GPU apaga a linha remota → re-INSERT manual (checklist
  README); o manager com `AUTO_ADOPT_LOCAL=0` não ressuscita o local sozinho.

## O que fica falso nos docs (lista para o `@docs-sync`, commit G.7)

**Não aplicar agora** — docs descrevem o que existe, não o que foi aprovado.

- `backend.md` §8/:104 — "Adoção (decisão: token colado)… Local: auto-adota via
  rede docker": a sessão GPU adota o remoto por **INSERT manual** (kind='remoto')
  como ponte até o adopt por token (dívida ADR-0009 D0) — nota de que o §8
  descreve o fluxo futuro; o v1 TrueNAS é o branch inbound direta do §1/:34.
- `backend.md` §10/:290 — "`gpus`/`vram_total_gb` NUNCA são escritos no v1":
  emenda — nunca escritos **pelo manager**; a sessão GPU os preenche por INSERT
  manual (dado estático do host, não heartbeat).
- `backend.md` §6/:89 — "manager aplica `medido * 1.25` e sugere atualizar o
  yaml": permanece futuro (policy no-op; D9) — nota do envelope 6GB.
- `backend.md` §9/:166-167 — telemetria: nota de que `gpus[]`/`vramUsed`/
  `vramTotal` agora **carregam valores reais** quando um orquestrador com GPU
  heartbeats (contrato inalterado).
- `docs/adr/0007-jobs-v1.md` D0 — "remoto/RunPod e transporte chunked FORA — o
  consumidor (orquestrador remoto) não existe na v1": **emenda** — o orquestrador
  remoto agora existe (TrueNAS, mesmo binário, LAN); o **chunked** (ADR-0003 D9)
  permanece futuro (LAN rápida). D5 "modo real @gpu documentado" → implementado
  (imagem gpu). D9 "telemetria sem GPU = no-op" → mantido para o local; o remoto
  preenche valores reais.
- `docs/adr/0009-web-integracao-monitoramento.md` D1 — "coluna vazia não é dado"
  → o INSERT manual da sessão GPU grava `gpus`/`vram_total_gb`; a regra "nunca
  escritos pelo manager" permanece.
- `frontend.md` — sem mudança de contrato; nota: o dashboard passa a mostrar
  gauges reais durante a sessão GPU (`items.length===1` — regra ADR-0009 D1
  cumprida pelo contrato operacional).
- `packages/policies/engines.yaml` — `toolchain: {cuda: 12.4, torch: 2.6.0,
  validated_at: <data do smoke>}` (registro da base validada — §4/:60);
  `vram-table.yaml` — nota da entrada `yolo11n=6` × 6GB físicos (revisão na fatia
  de policy).
- `dividas.md` — novas: "roteamento por capacidade no manager (GPU/VRAM/kind;
  substitui o LIMIT 1 + sessão GPU manual)"; "rotacionar credenciais S3/TLS fora
  da LAN" (reforço); nota "orchestrators.gpus/vram_total_gb preenchidos por
  INSERT manual na sessão GPU". Reafirmadas: telemetria por orquestrador,
  watchdog, adopt por token, policy VRAM aplicada.
- `coordenacao.md` — bloco da fatia G reescrito a cada commit.

## Plano de commits (G.0–G.7; branch `feat/treino-real-gpu` de `main`)

Um dispatch = um commit; produção < 400 linhas por commit (contrato/testes fora
da conta — exceção da casa). **G.1 ∥ G.2 ∥ G.3** (ownership disjunto: `engines/`
vs `services/orchestrator/` vs `services/manager/`); **G.4** depois de G.1/G.2
(o compose referencia `Dockerfile.gpu` e os envs); **G.5** review; **G.6** smoke
manual @gpu (coordenador, via SSH — NÃO é commit de código; eventuais fixes
viram commits próprios); **G.7** docs-sync.

| # | Dono | Conteúdo | Critério de pronto |
|---|---|---|---|
| **G.0** | @architect | Esta ADR (proposta; vira executável após aceite do usuário) | auditoria do coordenador; arquivo commitado em `main` |
| **G.1** | @python-engines | `train.py` caminho real completo (callback `on_train_epoch_end` → metrics.jsonl 6 keys incremental; copy flat best/last.pt; `device=0`/`seed`; conversão `mAP50(B)`/`mAP50-95(B)`) + `Dockerfile.gpu` (pytorch 2.6 cu124 runtime pinado + ultralytics pinado + `ENV ENGINE_MOCK=0` + bake `yolo11n.pt`) + pytest (ultralytics mockado, fixture metrics/results.csv) + README (seção gpu: imagem, build, envelope 6GB) | `pytest engines/trainer-yolo` verde (17 existentes + novos); mock path intocado (testes antigos passam); `docker build -f Dockerfile.gpu` local **não exigido** no CI (só lint do arquivo) |
| **G.2** | @rust-dev (orchestrator) | `TrainerExecutor::run` + `env`/`gpu_devices`; `DockerExecutor` (`--gpus device=…`, `--shm-size=2g`, `-e`; `None` → byte-a-byte atual); `run_job_inner` lê `ORCH_GPU_DEVICES` + guarda anti-mock (`ORCH_GPU_ALLOW_MOCK` escapa); heartbeat tenta nvidia-smi (parse puro com fixture, fallback silencioso) | `cargo test -p orchestrator` verde (novos: parse nvidia-smi, args do executor Some/None, guarda anti-mock); `cargo fmt --all`; ~250-350 linhas |
| **G.3** | @rust-dev (manager) | env `AUTO_ADOPT_LOCAL` (default `1`): boot não auto-adota `orchestrator-local` quando `0` + teste db (boot com `0` → 0 rows; default → 1 row) | `cargo test -p manager -- --ignored` + `bash scripts/test-db.sh` verdes; ~20 linhas |
| **G.4** | @infra-dev | `infra/compose.gpu.yaml` (projeto `gpu`: orchestrator-gpu build do clone + `gpus: all` + envs `MANAGER_URL`/`S3_ORCH_*`/`ORCH_GPU_DEVICES=1`/`ORCH_VOL_DATASETS=gpu_datasets`/`ORCH_VOL_OUTPUTS=gpu_outputs` + volumes `gpu_datasets`/`gpu_outputs`; serviço build-only `trainer-gpu` com `Dockerfile.gpu`) + `env.gpu.example` + `infra/README-gpu.md` (checklist completo da sessão GPU: publish S3 com `SEAWEED_PUBLISH=10.15.10.3` + force-recreate, `TRAINER_IMAGE=…:gpu` + recreate manager, stop local + DELETE row + INSERT remoto com gpus/vram_total_gb, pull/build/up no TrueNAS, **pre-flight nvidia-smi (3060 livre? senão `ORCH_GPU_DEVICES=1`)**, submit yolo11n batch 16 (3060), verificação, teardown) | `docker compose -f infra/compose.gpu.yaml config -q` no dev; README com todos os comandos executáveis; sem tocar `compose.yaml` (dev host intocado no default) |
| **G.5** | @reviewer | review do diff G.1–G.4 vs esta ADR (pontos de atenção: mock intocado, contrato flat do metrics.jsonl, guarda anti-mock, fallback silencioso da telemetria, sessão GPU no README) | APROVA (com ou sem nits); fixes roteados como commits próprios |
| **G.6** | @coordenador (ops manual, via SSH — fora do CI) | **Smoke @gpu no TrueNAS**: pre-flight (`nvidia-smi -L` no host; `curl http://10.15.1.2:8082/health` do dev; nvidia-smi dentro do container do orquestrador); sessão (D2/D3/D4); build `:gpu` no TrueNAS; submit via API/UI `yolo11n epochs=3 batch=8 imgsz=640`; verificação binária (abaixo) | critérios: (1) job vai para o remoto (`orchestrators` = 1 row `remoto`; `job.orchestrator_id` = uuid remoto); (2) GPU usada = a escolhida no pre-flight (default 3060; `nvidia-smi` durante o run mostra util>0 nela e ~0 na outra; log ultralytics "Using device 0"); (3) artefatos reais (`best.pt` > 1MB, md5 varia entre runs — ≠ 110 bytes HEPHMOCK); (4) `GET /api/telemetry` com `gpus[]` reais e `vramUsed/vramTotal` > 0; dashboard com gauges reais; (5) **falha honesta**: 2º job `yolo11x` (definitivamente > 12GB) → `failed` com log OOM (não silencioso); (6) teardown: down remoto, restore dev host, DELETE row local/remoto + restart manager (re-adota local) |
| **G.7** | @docs-sync | Aplica "O que fica falso nos docs": backend.md §8/§10/§6/§9, emendas ADR-0007 (D0/D5/D9) e ADR-0009 (D1), `engines.yaml` (toolchain validado), vram-table nota, dividas.md (novas dívidas), coordenacao.md | diff só de docs; conferência doc↔código nos dois sentidos (lição sessão 16) |

**Notas de processo:** nenhum dos passos G.1–G.3 pode quebrar o caminho mock —
o critério "mock intocado" é testado por baterias existentes (pytest 17, cargo
orchestrator, contract 0.9.0). O G.6 é manual por decisão da casa (teste @gpu
fora do CI — FATO spike). Implementador que achar problema FORA do escopo para e
reporta ao coordenador (norma das fatias 4/5). `cargo fmt --all` antes de
reportar. O smoke G.6 usa portas efêmeras/altas e NÃO toca os 48 containers do
TrueNAS (nada de `docker compose down` global; só o projeto `gpu`).
