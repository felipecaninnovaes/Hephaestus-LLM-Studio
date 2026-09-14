# ADR-0022 — Nomeação Semântica Automática, Configuração de Nome e Renomeação de Modelos

- **Status:** ACEITA
- **Data:** 2026-09-13
- **Componentes:** `packages/contracts` (OpenAPI 0.21.0 → 0.22.0), `services/manager`, `services/api-principal`, `apps/web`.

## Contexto

Quando um job de treinamento termina com sucesso, o orquestrador gera o arquivo de modelo no diretório do job com um nome fixo e genérico:
- Treinos de difusão LoRA: `adapter.safetensors`
- Treinos YOLO: `best.pt`

O hook do `manager` registra automaticamente esse artefato na tabela `models` utilizando o nome extraído do caminho (`split_part(path, '/', -1)`). Como consequência:
1. No catálogo de modelos (`/models`), todos os adaptadores LoRA aparecem com o título idêntico `adapter.safetensors` e todos os modelos YOLO com `best.pt`.
2. No seletor de LoRA do Playground (`/playground`), o menu suspenso exibe múltiplos itens indistinguíveis chamados `adapter.safetensors`.
3. Ao baixar os pesos através do estúdio, o navegador recebe arquivos duplicados (`adapter.safetensors`, `adapter (1).safetensors`), exigindo renomeação manual e verificação de metadados para uso externo (ComfyUI, Forge, etc.).

## Decisões

### D0 — Nome Semântico Automático (Default Inteligente)
Ao registrar o artefato final na tabela `models` após a conclusão do job (`report_job` no `manager`):
- Se o usuário tiver fornecido `output_name` no job, utiliza-o com a extensão correspondente (`{output_name}.{ext}`).
- Caso contrário, deriva um nome semântico legível:
  $$\text{Difusão: } \texttt{\{dataset\_slug\}-\{base\_model\}-\{trigger\_word ou short\_id\}.safetensors}$$
  $$\text{YOLO: } \texttt{\{dataset\_slug\}-\{model\}-best.pt}$$
- O nome é sanitizado para caracteres seguros (`[a-z0-9_-]`).

### D1 — Parâmetro Opcional `outputName` na Criação de Jobs
- Em `DiffusionJobRequest` e `YoloJobRequest`, é aceito o campo opcional `outputName: string` (1 a 100 caracteres, slug-safe).
- O campo é validado na API Principal e repassado no payload de criação para o Manager armazenar em `jobs.params`.

### D2 — Endpoint de Atualização de Modelo (`PATCH /api/models/{id}`)
- Adicionado endpoint no catálogo de modelos para permitir que o usuário renomeie qualquer modelo já existente:
  - `PATCH /api/models/{id}` com corpo `{"name": "novo-nome.safetensors"}`.
  - A API Principal valida o token de autenticação e delega para o Manager (`PATCH /internal/models/:id`).
  - O Manager atualiza o campo `name` na tabela `models`.

### D3 — Download de Artefatos com Nome Descritivo
- No endpoint de download de artefato (`GET /api/jobs/{id}/artifacts/{artifactId}/data`) ou no cliente web, o cabeçalho HTTP `Content-Disposition` e a rotina de download utilizam o nome semântico ou o nome registrado do modelo quando o artefato for do tipo `model`.

### D4 — Interface Web: Campo de Nome no Setup e Ação de Renomear no Catálogo
- Em `ForjaDifusaoSetup` e `ForjaYoloSetup`, é adicionado o campo "Nome do Modelo / Adaptador" com sugestão dinâmica inteligente.
- Na página `/models`, é disponibilizada a opção de renomear modelos diretamente no card da interface.
