"""
Backend real OpenCLIP (ViT-B-32 / laion2b_s34b_b79k) com aceleração CUDA opcional.
"""
from __future__ import annotations

from typing import Any


def load_real_clip(model_name: str = "ViT-B-32") -> dict[str, Any]:
    """Carrega o modelo OpenCLIP e o preprocessador na GPU se disponível."""
    import open_clip
    import torch

    device = "cuda" if torch.cuda.is_available() else "cpu"
    pretrained = "laion2b_s34b_b79k" if model_name == "ViT-B-32" else None
    model, _, preprocess = open_clip.create_model_and_transforms(
        model_name, pretrained=pretrained, device=device
    )
    model.eval()
    tokenizer = open_clip.get_tokenizer(model_name)
    return {
        "model": model,
        "preprocess": preprocess,
        "tokenizer": tokenizer,
        "device": device,
        "torch": torch,
    }


def real_embed_images(state: dict[str, Any], payloads: list[bytes]) -> list[list[float]]:
    """Gera embeddings para lista de payloads de imagem binária."""
    import io
    from PIL import Image

    torch = state["torch"]
    device = state["device"]
    preprocess = state["preprocess"]
    model = state["model"]

    tensors = []
    for b in payloads:
        im = Image.open(io.BytesIO(b)).convert("RGB")
        tensors.append(preprocess(im))
    batch = torch.stack(tensors).to(device)

    with torch.no_grad():
        feats = model.encode_image(batch)
        feats = feats / feats.norm(dim=-1, keepdim=True)
    return feats.cpu().tolist()


def real_embed_texts(state: dict[str, Any], texts: list[str]) -> list[list[float]]:
    """Gera embeddings para lista de textos."""
    torch = state["torch"]
    device = state["device"]
    tokenizer = state["tokenizer"]
    model = state["model"]

    tokens = tokenizer(texts).to(device)
    with torch.no_grad():
        feats = model.encode_text(tokens)
        feats = feats / feats.norm(dim=-1, keepdim=True)
    return feats.cpu().tolist()
