# Trainer YOLO (`engines/trainer-yolo`)

O `trainer-yolo` é o motor especializado em visão computacional do Hephaestus, provendo funcionalidades de treinamento supervisionado da arquitetura YOLO11, anotação automatizada assistida por modelos de linguagem e visão (Autolabel via VLM/OpenAI) e rastreamento contínuo de objetos (Autotrack).

Assim como os demais motores, suporta execução acelerada por GPU e modo offline determinístico (`ENGINE_MOCK=1`).

---

## Módulos e Componentes

### 1. `yolo_adapter.py`
Isola completamente a dependência pesada da biblioteca `ultralytics`:
- **`get_yolo_class()`**: Executa importação tardia (*lazy import*) da classe `YOLO`. Caso o pacote não esteja instalado ou falte suporte CUDA em tempo de execução real, aborta graciosamente via `engine_kit.runtime.die`.
- Permite que o código-fonte do motor seja importado e testado em ambientes sem Ultralytics.

### 2. `config.py`
Gerencia a validação e o esquema de parâmetros dos jobs:
- **`load_and_validate_config(path)`**: Valida a integridade do arquivo `config.yaml` contra os esquemas requeridos de treino, predição e autotrack.
- Checa parâmetros estruturais (`job_id`, `engine`, `model`, `dataset_path`, `output_path`) e hiperparâmetros de treinamento (`epochs`, `batch`, `imgsz`, `lr0`, `optimizer`, data augmentation com `mosaic` e `mixup_flip`).

### 3. `deterministic.py`
Simulação completa do comportamento do YOLO para modo mock e CI:
- **`_synthetic_metrics(...)`**: Emite progressão matemática de métricas de treino (`box_loss`, `cls_loss`, `dfl_loss`, `mAP50`, `mAP50-95`) sincronizadas com `seed_bytes`.
- **`_make_fake_artifact(...)`**: Gera binários `.pt` sintéticos assinados com a constante mágica `HEPHMOCK` para validação de pipeline pelo orchestrator.
- Gera coordenadas de *bounding boxes* reprodutíveis para modos de autolabel e inferência simulados.

### 4. `autolabel_pkg/`
Subpacote dedicado à anotação automática e geração de descrições visuais:
- **`captions.py`**: Suporte para inferência de legendas utilizando VLMs locais (como Florence-2 e Qwen2-VL) ou dicionários determinísticos no modo mock.
- **`vision_api.py`**: Cliente de integração com APIs remotas compatíveis com OpenAI Vision (`/chat/completions`), suportando provedores externos ou instâncias locais (vLLM, Ollama, LM Studio).
- **`pipeline.py`**: Orquestrador que itera sobre as amostras do dataset, codifica imagens em Base64, submete aos modelos e persiste os metadados gerados emitindo telemetria contínua.
  - **Modo `openai` concorrente**: as chamadas à Vision API rodam num `ThreadPoolExecutor` com janela de até `AUTOLABEL_CONCURRENCY` requisições em voo (default 1; mínimo 1; valor inválido cai em 1). O default é 1 porque servidores llama.cpp com speculative decoding retornam HTTP 500 sob requisições simultâneas (ggml-org/llama.cpp#24840). O `captions.jsonl` é gravado na ordem ordenada dos arquivos (só o prefixo contíguo já resolvido é descarregado; imagens puladas contam como resolvidas), então a saída não depende da ordem de chegada das respostas.
  - **Variáveis `AUTOLABEL_*`**: são lidas pelo engine, mas o orchestrator não as repassa ao container do job; na prática valem os defaults (mudá-los exige rebuild da imagem).
  - **Imagens puladas**: imagem que falha após os retries (HTTP 5xx/429, conexão, resposta vazia ou truncada) é pulada: não entra em `captions.jsonl`, vai para `autolabel_failures.jsonl` (`{filename, error}`, só fica no diretório de saída do engine; o orchestrator não sobe esse arquivo) e a mensagem final informa `X/Y imagens anotadas; Z puladas por erro`.
  - **Aborto do job** (`_die`): HTTP 401/403/404 (erro de configuração de chave/endpoint/modelo), `AUTOLABEL_MAX_CONSECUTIVE_FAILURES` falhas seguidas (default 10) ou nenhuma imagem anotada.
  - **Orçamento de tokens e timeout**: por padrão nenhum `max_tokens` é enviado (vale o limite do servidor), necessário para que modelos com raciocínio concluam a resposta. `AUTOLABEL_MAX_TOKENS` (inteiro positivo, opcional; valor inválido é ignorado) é enviado como `max_tokens`. `AUTOLABEL_REQUEST_TIMEOUT` define o timeout em segundos por requisição (default 300).
  - **Raciocínio nunca vira legenda**: blocos `<think>...</think>` são removidos do `content` (um `<think>` sem fechamento descarta o restante) e campos de raciocínio (`reasoning_content` etc.) nunca são usados. Resposta final vazia ou `finish_reason=length` gera erro com mensagem explícita (sugere aumentar `AUTOLABEL_MAX_TOKENS`/contexto do servidor ou desativar o raciocínio); a imagem é pulada (ver acima), não derruba o job sozinha.

---

## Modos Operacionais

1. **Treino YOLO11 (`train.py`)**:
   - Suporta tarefas de detecção de objetos (Bounding Boxes) e segmentação de instâncias (Polígonos).
   - Exporta artefatos finais em formato `.pt`, acompanhados de gráficos de métricas e checkpoints intermediários.
2. **Autolabel (`autolabel.py`)**:
   - Geração acelerada de rótulos e descrições sem necessidade de anotação manual prévia.
3. **Autotrack (`autotrack.py`)**:
   - Rastreamento e associação de objetos ao longo de sequências de quadros, gerando caixas delimitadoras com IDs persistentes.

---

## Contratos e Políticas

- Parâmetros de entrada e saída são canônicos em `packages/contracts/openapi.yaml`.
- Limites de memória de GPU e alocações de lote por resolução são canônicos em `packages/policies/vram-table.yaml`.

---

## Adicionando um Novo Modelo

Para o checklist transversal dos 4 Pilares ao introduzir novas variantes de modelos, consulte:
- [`docs/engines/novo-modelo.md`](novo-modelo.md)
