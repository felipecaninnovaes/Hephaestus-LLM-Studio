# Trainer CLIP (`engines/trainer-clip`)

O `trainer-clip` é o micro-serviço de inferência de embeddings multimodais do Hephaestus. Ele expõe um daemon HTTP de baixa latência responsável por gerar vetores semânticos densos a partir de imagens e textos, alimentando a busca semântica de datasets no `api-principal` e a indexação vetorial.

---

## Arquitetura e Componentes

O serviço foi estruturado para ser leve e resiliente, operando em porta dedicada (padrão `8090`) através de `engine_kit.httpd.run_daemon`:

### 1. `server.py` e Endpoints HTTP
Servidor baseado em `ThreadingHTTPServer` com serialização JSON:
- **`GET /health`**: Retorna o status de prontidão (`status: ok`), o modo operacional (`mode: clip` ou `mock`), a dimensão vetorial (`dim: 512`) e o modelo configurado.
- **`POST /embed`**: Processa uma lista de itens com identificador e imagem codificada em Base64 (`items: [{ id, b64 }]`), devolvendo os vetores normalizados L2 (`items: [{ id, vector, dim }]`).
- **`POST /embed-text`**: Processa uma lista de strings textuais (`texts: [...]`), devolvendo os embeddings correspondentes para comparação multimodal.

### 2. `clip_backend.py` (Modo Real)
Backend para execução acelerada em GPU:
- Realiza *lazy import* das bibliotecas `torch`, `torchvision` e `open_clip`.
- Carrega sob demanda o modelo base canônico e suas transformações de pré-processamento de imagem.
- Executa inferência em batch com projeção normalizada no espaço latente.

### 3. `mock_embed.py` (Modo Mock)
Backend determinístico para desenvolvimento local em CPU e testes de integração:
- Ativado quando `ENGINE_MOCK=1` (padrão em ambiente de desenvolvimento).
- Gera vetores normalizados L2 de 512 dimensões via hash criptográfico determinístico através de `engine_kit.mock.mock_vector`.
- Permite que o fluxo de busca semântica e deduplicação do estúdio funcione integralmente sem requerer placas de vídeo dedicadas ou downloads de modelos de vários gigabytes.

---

## Modelo Canônico e Configuração

- **Arquitetura Padrão**: `ViT-B-32` (dimensão vetorial canônica de 512 números em ponto flutuante).
- **Variáveis de Ambiente**:
  - `PORT`: Porta de escuta do daemon (padrão: `8090`).
  - `MODEL`: Identificador do modelo CLIP (padrão: `ViT-B-32`).
  - `ENGINE_MOCK`: Chave booleana que alterna entre simulação determinística e inferência real.

---

## Contratos e Políticas

- Os esquemas dos endpoints e tipos das requisições são canônicos em `packages/contracts/openapi.yaml`.
- Os requisitos de memória gráfica para o serviço CLIP são definidos em `packages/policies/vram-table.yaml`.
