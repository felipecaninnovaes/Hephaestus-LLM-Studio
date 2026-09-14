"""Dataset de imagens e prompts para fine-tuning de modelos de difusão."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from trainer_difusao.common import _die


class DiffusionDataset:
    """Dataset para leitura de pares imagem (.webp, .png, .jpg, .jpeg) + legenda (.txt)."""

    def __init__(
        self, dataset_path: Path, resolution: int = 512, trigger_word: str = ""
    ):
        self.samples: list[tuple[Path, str]] = []
        self.resolution = resolution
        self.trigger_word = trigger_word.strip()

        # Busca em images/ ou na raiz do dataset
        target_dir = dataset_path / "images"
        if not target_dir.exists():
            target_dir = dataset_path

        valid_exts = {".webp", ".png", ".jpg", ".jpeg"}
        if target_dir.exists():
            for p in sorted(target_dir.iterdir()):
                if p.suffix.lower() in valid_exts:
                    txt_path = p.with_suffix(".txt")
                    caption = ""
                    if txt_path.exists():
                        caption = txt_path.read_text(encoding="utf-8").strip()
                    if not caption and self.trigger_word:
                        caption = self.trigger_word
                    self.samples.append((p, caption))

        if not self.samples:
            _die(f"Nenhuma imagem encontrada para treino em: {target_dir}")

    def __len__(self) -> int:
        return len(self.samples)

    def __getitem__(self, idx: int) -> dict[str, Any]:
        import numpy as np
        import torch
        from PIL import Image

        img_path, caption = self.samples[idx]
        image = Image.open(img_path).convert("RGB")
        image = image.resize(
            (self.resolution, self.resolution), Image.Resampling.BILINEAR
        )

        # Normaliza para [-1.0, 1.0]
        img_np = (np.array(image, dtype=np.float32) / 127.5) - 1.0
        # HWC -> CHW
        img_tensor = torch.from_numpy(img_np).permute(2, 0, 1)

        return {"pixel_values": img_tensor, "prompt": caption}
