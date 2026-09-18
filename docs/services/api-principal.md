# Serviço: api-principal

O `api-principal` é o gateway e Backend For Frontend (BFF) do Hephaestus LLM Studio, exposto na porta `:8080`.

## Papel e Responsabilidades

- **BFF Unificado:** Ponto de entrada HTTP único para o navegador e clientes externos, orquestrando requisições de domínio entre o banco relacional e os serviços internos.
- **Terminação de Autenticação:**
  - Gerenciamento de credenciais mestre (`STUDIO_PASSWORD`, `STUDIO_MASTER_KEY`).
  - Suporte a cookie de sessão seguro (`heph_session` com flags `HttpOnly`, `SameSite=Lax`, e `Secure` em conexões TLS).
  - Emissão e validação de tokens JWT (assinados com segredo de 64 caracteres hex via `AUTH_SECRET`).
- **Validação e Normalização de Entrada:**
  - Sanitização de metadados de datasets, nomes de classes e tags.
  - Validação estrita de imagens enviadas (dimensões, formatos suportados e checagem de MD5).

## Endpoints e Contratos de API

A lista completa de endpoints, parâmetros de requisição e esquemas de resposta está formalmente definida em:
- **`packages/contracts/openapi.yaml`**

Os principais domínios atendidos pelo `api-principal` incluem:
- `/api/auth/*`: Login, logout, status da sessão e bootstrap de senha inicial.
- `/api/datasets/*`: Criação de datasets, CRUD de classes, upload em lote, soft-delete e exportação/importação de snapshots.
- `/api/models/*`: Registro, download remoto, uploads e inspeção de checkpoints.
- `/api/jobs/*`: Submissão de treinamentos (YOLO e Difusão), cancelamento e telemetria.
- `/api/search/*`: Busca semântica vetorial integrada a embeddings CLIP.
- `/api/generations/*`: Histórico e galeria de imagens geradas por difusão.

## Protocolo de Upload Chunked

Arquivos de checkpoints e pesos de modelos frequentemente excedem centenas de megabytes ou gigabytes. O proxy reverso do Next.js acumula multipartes em buffer de memória, o que pode causar Out-Of-Memory (OOM) em uploads grandes.

Para solucionar isso, o `api-principal` implementa um protocolo modular de upload chunked:

1. **Inicialização (`POST /api/models/uploads/init`):**
   - Cria uma sessão em memória (`uploadId`), validando nome, arquitetura pretendida e tamanho total.
   - Aloca um diretório temporário isolado (`tempfile::TempDir`) em disco.
2. **Envio de Partes (`PUT /api/models/uploads/:uploadId/part/:partNumber`):**
   - Cada parte é transmitida como corpo binário bruto (`application/octet-stream`) de até 96 MiB (`CHUNK_PART_SIZE`).
   - O payload faz streaming direto do socket de rede para arquivo temporário no disco, mantendo pegada de RAM $O(1)$.
3. **Conclusão (`POST /api/models/uploads/:uploadId/complete`):**
   - Valida a integridade de todas as partes, concatena os pedaços em disco, calcula o hash MD5 canônico e registra o modelo no catálogo persistido.
4. **Abort e Limpeza (`DELETE /api/models/uploads/:uploadId`):**
   - Permite cancelamento explícito. No descarte da sessão (`drop`), o `TempDir` é sumariamente removido do disco. Uma rotina periódica de sweep remove sessões inativas abandonadas.

## Telemetria Reativa via Server-Sent Events (SSE)

Para exibir o progresso de jobs de treinamento na interface sem polling excessivo no banco:

- **Canal SSE (`GET /api/jobs/:id/telemetry`):** Estabelece um stream persistente de eventos de texto (`text/event-stream`).
- **Filtragem de Mudança de Estado:** O handler compara amostras sucessivas (`progress`, `phase`, `step`, `epoch`, `vram_used_gb`) e só despacha eventos quando há mutação observável real.
- **Ciclo de Vida:** O stream emite o estado inicial de imediato e é finalizado automaticamente com evento terminal quando o job atinge `done`, `failed` ou `cancelled`.
