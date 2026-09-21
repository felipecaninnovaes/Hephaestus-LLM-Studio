"""Dataset de imagens e prompts para fine-tuning de modelos de difusão."""

from __future__ import annotations

import random
from pathlib import Path
from typing import Any

from trainer_difusao.common_pkg.metrics import _emit_metric

# Passo de alinhamento das dimensões do bucket (exigência dos VAEs de difusão).
_BUCKET_STEP = 64


def _resolve_bucket_reso(
    width: int, height: int, base_res: int, step: int = _BUCKET_STEP
) -> tuple[int, int]:
    """Resolve a resolução (w, h) de bucket mais próxima do aspect ratio da imagem.

    Preserva a proporção original da imagem mantendo a área próxima de
    ``base_res²`` e os dois lados múltiplos de ``step`` (64px), requisito dos
    VAEs (ex.: FLUX e SDXL) para a compressão latente. Usada pelo bucketing
    por aspect ratio (``enable_bucket``).
    """
    area = float(base_res * base_res)
    min_side = max(step, base_res // 2)
    max_side = max(min_side, base_res * 2)
    target_ar = width / max(1, height)

    best = (base_res, base_res)
    best_diff = float("inf")
    h = min_side
    while h <= max_side:
        w = int(area / h)
        w = max(min_side, min(max_side, round(w / step) * step))
        if w < min_side or w > max_side:
            h += step
            continue
        ar = w / h
        diff = abs(ar - target_ar)
        if diff < best_diff:
            best_diff = diff
            best = (w, h)
        h += step
    return best


class DiffusionDataset:
    """Dataset para leitura de pares imagem (.webp, .png, .jpg, .jpeg) + legenda (.txt).

    Com ``enable_bucket=True`` as amostras são agrupadas em buckets de aspect
    ratio (área ≈ ``resolution²``, lados múltiplos de 64) para preservar a
    proporção original sem distorção e sem estourar VRAM.
    """

    def __init__(
        self,
        dataset_path: Path,
        resolution: int = 512,
        trigger_word: str = "",
        enable_bucket: bool = False,
        empty_captions: bool = False,
        metrics_path: Path | None = None,
    ):
        """Inicializa o dataset (com ``empty_captions`` a legenda é sempre "" — controle/regularização)."""
        self.samples: list[tuple[Path, str]] = []
        self.resolution = resolution
        self.trigger_word = trigger_word.strip()
        self.enable_bucket = bool(enable_bucket)
        self.empty_captions = bool(empty_captions)
        # Dimensões (w, h) efetivas de cada amostra: bucket resolvido ou quadrado.
        self.bucket_dims: list[tuple[int, int]] = []
        # bucket (w, h) -> índices das amostras naquele bucket.
        self.buckets: dict[tuple[int, int], list[int]] = {}

        # Busca em images/ ou na raiz do dataset
        target_dir = dataset_path / "images"
        if not target_dir.exists():
            target_dir = dataset_path

        valid_exts = {".webp", ".png", ".jpg", ".jpeg"}
        if target_dir.exists():
            for p in sorted(target_dir.iterdir()):
                if p.suffix.lower() in valid_exts:
                    if self.empty_captions:
                        self.samples.append((p, ""))
                        continue
                    txt_path = p.with_suffix(".txt")
                    caption = ""
                    if txt_path.exists():
                        caption = txt_path.read_text(encoding="utf-8").strip()
                    if not caption and self.trigger_word:
                        caption = self.trigger_word
                    self.samples.append((p, caption))

        if not self.samples:
            _die(f"Nenhuma imagem encontrada para treino em: {target_dir}")

        if self.enable_bucket:
            self._build_buckets()
        else:
            self.bucket_dims = [(resolution, resolution)] * len(self.samples)
        if metrics_path is not None:
            scope = " (controle)" if self.empty_captions else ""
            _emit_metric(
                metrics_path,
                epoch=0,
                step=0,
                progress=0.07,
                phase="preparing_dataset",
                message=(f"Preparando dataset{scope}: {len(self.samples)} imagens encontradas."),
                telemetry_only=True,
            )

    def _build_buckets(self) -> None:
        from PIL import Image

        for i, (path, _) in enumerate(self.samples):
            with Image.open(path) as im:
                w, h = im.size
            bw, bh = _resolve_bucket_reso(w, h, self.resolution)
            self.bucket_dims.append((bw, bh))
            self.buckets.setdefault((bw, bh), []).append(i)

    def __len__(self) -> int:
        return len(self.samples)

    def __getitem__(self, idx: int) -> dict[str, Any]:
        import numpy as np
        import torch
        from PIL import Image

        img_path, caption = self.samples[idx]
        w, h = self.bucket_dims[idx]
        image = Image.open(img_path).convert("RGB")
        image = image.resize((w, h), Image.Resampling.BILINEAR)

        # Normaliza para [-1.0, 1.0]
        img_np = (np.array(image, dtype=np.float32) / 127.5) - 1.0
        # HWC -> CHW
        img_tensor = torch.from_numpy(img_np).permute(2, 0, 1)

        return {"pixel_values": img_tensor, "prompt": caption}


class BucketBatchSampler:
    """Agrupa amostras por bucket para que cada batch tenha resolução uniforme.

    Necessário quando ``enable_bucket=True`` e ``batch_size > 1``: o collate
    do DataLoader exige tensores de mesma forma dentro do batch. Amostras de
    buckets diferentes nunca se misturam; restos de bucket viram batches
    menores (nada é descartado). Ordem aleatória determinística por semente.
    """

    def __init__(self, dataset: DiffusionDataset, batch_size: int, seed: int | None = None):
        if batch_size < 1:
            raise ValueError("batch_size must be >= 1")
        self.dataset = dataset
        self.batch_size = batch_size
        self._rng = random.Random(seed)

    def __iter__(self):
        buckets = list(self.dataset.buckets.values())
        self._rng.shuffle(buckets)
        batches: list[list[int]] = []
        for bucket in buckets:
            indices = list(bucket)
            self._rng.shuffle(indices)
            for i in range(0, len(indices), self.batch_size):
                batches.append(indices[i : i + self.batch_size])
        self._rng.shuffle(batches)
        return iter(batches)

    def __len__(self) -> int:
        return sum(
            (len(indices) + self.batch_size - 1) // self.batch_size
            for indices in self.dataset.buckets.values()
        )


def build_dataloader(
    dataset: DiffusionDataset, batch_size: int, seed: int | None = None
) -> Any:
    """Constrói o DataLoader respeitando o modo de bucketing do dataset."""
    from torch.utils.data import DataLoader

    if dataset.enable_bucket:
        return DataLoader(
            dataset, batch_sampler=BucketBatchSampler(dataset, batch_size, seed=seed)
        )
    return DataLoader(dataset, batch_size=batch_size, shuffle=True, drop_last=False)