# trainer-yolo

Engine de treino YOLO para o Hephaestus. Suporta dois modos:

## Modo mock (ENGINE_MOCK=1, default)

O modo padrão. Não requer GPU nem `ultralytics`. Simula treino com métricas
sintéticas determinísticas e grava artefatos fake (`best.pt`, `last.pt`).

**Uso (via CLI):**

```bash
python -m trainer_yolo train --config <config.yaml> --output <dir>
```

**Uso (Docker):**

```bash
docker run --rm \
  -v /path/to/config.yaml:/config.yaml:ro \
  -v /path/to/output:/output \
  hephaestus/trainer-yolo:local \
  train --config /config.yaml --output /output
```

A variável `MOCK_EPOCH_SLEEP_MS` controla o sleep entre epochs (default 200ms).
Defina `MOCK_EPOCH_SLEEP_MS=0` para testes rápidos.

## Modo real @gpu (via Dockerfile.gpu)

O modo real usa [ultralytics](https://docs.ultralytics.com/) e requer GPU
compatível com CUDA. A imagem `hephaestus/trainer-yolo:gpu` é construída
a partir de `Dockerfile.gpu` e tem `ENGINE_MOCK=0` baked.

### Build da imagem GPU

```bash
cd engines/trainer-yolo
docker build -f Dockerfile.gpu -t hephaestus/trainer-yolo:gpu .
```

A imagem inclui:
- Base `pytorch/pytorch:2.6.0-cuda12.4-cudnn9-runtime`
- `ultralytics==8.3.253` (pin exato)
- Peso `yolo11n.pt` baked na camada (sem dependência de rede no runtime)

### Execução via Docker

```bash
docker run --rm --gpus device=0 \
  -v /path/to/config.yaml:/config.yaml:ro \
  -v /path/to/output:/output \
  hephaestus/trainer-yolo:gpu \
  train --config /config.yaml --output /output
```

### Envelope VRAM (referência)

| GPU | VRAM | Modelo | Batch max | Notas |
|-----|------|--------|-----------|-------|
| RTX 3060 | 12GB | yolo11n | 16 | envelope seguro (default) |
| RTX 3060 | 12GB | yolo11m | 8 | ~10-11GB, no limite |
| GTX 1660S | 6GB | yolo11n | 8 | fallback; yolo11m OOM |
| GTX 1660S | 6GB | yolo11m | — | OOM (falha honesta, job failed) |

**Nota:** o compose do TrueNAS (`infra/compose.gpu.yaml`, G.4) é quem orquestra
a sessão GPU — veja `infra/README-gpu.md` para o checklist completo.

### Setup local (desenvolvimento, sem Docker)

1. Instale com extras `[train]`:

```bash
cd engines/trainer-yolo
pip install -e '.[train]'
```

Ou com `uv`:

```bash
uv pip install -e '.[train]'
```

2. Desative o mock:

```bash
export ENGINE_MOCK=0
```

### Execução local

```bash
python -m trainer_yolo train \
  --config path/to/config.yaml \
  --output path/to/output
```

O `config.yaml` deve seguir o formato:

```yaml
job_id: "my-job-001"
engine: "yolo"
model: "yolo11m"
mode: "train"
dataset_path: "/path/to/dataset"
output_path: "/path/to/output"
seed: 42

yolo:
  model: "yolo11m"
  epochs: 100
  batch: 16
  imgsz: 640
  lr0: 0.01
  optimizer: "AdamW"
  augment:
    mosaic: true
    mixup_flip: false
```

### Equivalente via CLI ultralytics

O modo real é um wrapper em torno do `yolo train`:

```bash
yolo train \
  data=/path/to/dataset/dataset.yaml \
  model=yolo11m \
  epochs=100 \
  batch=16 \
  imgsz=640 \
  lr0=0.01 \
  optimizer=AdamW \
  project=/path/to/output \
  name=train
```

### Saída

- `metrics.jsonl` — uma linha JSON por epoch com keys:
  `epoch`, `box_loss`, `cls_loss`, `dfl_loss`, `mAP50`, `mAP50-95`
- `best.pt` — melhor checkpoint (flat no output)
- `last.pt` — último checkpoint (flat no output)

## Format (contrato com o orquestrador)

O orquestrador (F4.4) captura o stdout e lê `metrics.jsonl` para enviar ao
manager como `metrics` (D4 da ADR-0007). Cada linha JSON do metrics.jsonl
contém exatamente 6 keys: `epoch`, `box_loss`, `cls_loss`, `dfl_loss`,
`mAP50`, `mAP50-95`.
