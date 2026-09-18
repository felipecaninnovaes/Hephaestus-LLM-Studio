# Serviço: manager

O `manager` é o serviço central de controle assíncrono, persistência relacional de tarefas e gerenciamento de nós de computação, operando na porta `:8081`.

## Papel e Responsabilidades

- **Gerenciador de Estado:** Mantém o registro canônico de jobs, nós de computação (orquestradores) e políticas de agendamento no PostgreSQL.
- **Isolamento e Segurança:** Não é exposto à internet pública; atua como serviço de retaguarda acessado apenas pelo `api-principal` e pelos nós de computação autenticados via token de segurança (`MANAGER_TOKEN`).
- **Coordenador de Fila e Despacho:** Avalia periodicamente jobs pendentes na fila e os despacha para os nós de execução que atendem aos requisitos de hardware e VRAM.
- **Contratos:** Rotas internas e modelos de dados estão referenciados em `packages/contracts/openapi.yaml`.

## Ciclo de Vida de Jobs

O ciclo de vida dos jobs segue transições de estado bem definidas e auditáveis:

```
[queued] ──> [preparing] ──> [running] ──> [done]
   │             │               │
   v             v               v
[cancelled]   [failed]        [failed]
```

1. **`queued`:**
   - O job foi aceito e registrado no banco de dados.
   - Aguarda alocação de recursos (nó online com VRAM suficiente conforme `packages/policies/vram-table.yaml`).
2. **`preparing`:**
   - O `api-principal` ou rotina dedicada empacota os dados necessários (ex: snapshots de imagens, anotações em formato YOLO ou manifest de difusão) e assegura que os artefatos de entrada estejam no S3.
   - O job é protegido contra execução prematura antes da finalização do pacote.
3. **`running`:**
   - O `manager` despacha o job para o `orchestrator` eleito (`POST /internal/dispatch`).
   - O nó confirma o recebimento e reporta progresso continuamente via `POST /internal/report`.
4. **Estados Terminais (`done`, `failed`, `cancelled`):**
   - **`done`:** Treinamento ou processamento concluído com sucesso e artefatos de saída gravados no S3.
   - **`failed`:** Erro fatal no script do container, falha de infraestrutura ou estouro de timeout.
   - **`cancelled`:** Interrupção voluntária solicitada pelo usuário na UI.

## Watchdogs de Health e Recuperação de Zumbis

Para garantir que falhas parciais de rede ou quedas abruptas de servidores não bloqueiem permanentemente a fila de execução, o `manager` executa processos concorrentes de reconciliação:

### 1. Heartbeats de Nós e Telemetria
- Cada `orchestrator` envia um heartbeat periódico (`POST /internal/heartbeat`) contendo:
  - Carga atual de CPU, memória RAM e uso de disco.
  - Inventário de GPUs (modelo, VRAM total e VRAM livre instantânea).
  - Quantidade de jobs em execução localmente.
- O `manager` atualiza `last_heartbeat` e armazena o status em cache de telemetria.
- Nós sem heartbeat recente (> 10 segundos) são considerados `stale` (offline) e não recebem novos dispatches de jobs.

### 2. Watchdog de Jobs Zumbis (Stale Recovery)
- **Timeout de Preparação:** Jobs que permanecem no estado `preparing` por mais de 60 minutos sem progresso têm sua preparação considerada morta e são movidos para `failed` com o erro `prepare_timeout`.
- **Jobs Órfãos em Execução:** Caso um nó de computação deixe de responder ou seja reiniciado com jobs em `running`, o watchdog reconcilia a discrepância marcando jobs sem nós correspondentes como `failed`.
- **Prevenção de Falso Positivo:** O timeout é dimensionado para respeitar os tempos reais de transferência de pesos pesados (como modelos FLUX ou SDXL) e snapshots grandes via S3.
