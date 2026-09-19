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
