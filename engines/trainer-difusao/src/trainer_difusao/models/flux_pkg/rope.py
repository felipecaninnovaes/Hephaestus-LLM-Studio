"""
Operações tensoriais de packing, patchify e cálculo de coordenadas RoPE para FLUX.1 e FLUX.2.
"""
from __future__ import annotations

from typing import Any


def _pack_latents(latents: Any) -> Any:
    """Empacota tensores latentes do VAE no formato patch 2x2 do FLUX.1: [B, C, H, W] -> [B, (H//2)*(W//2), C*4]."""
    b, c, h, w = latents.shape
    latents = latents.view(b, c, h // 2, 2, w // 2, 2)
    latents = latents.permute(0, 2, 4, 1, 3, 5)
    latents = latents.reshape(b, (h // 2) * (w // 2), c * 4)
    return latents


def _patchify_latents_flux2(latents: Any) -> Any:
    """Aplica patchify 2x2 nos latentes do FLUX.2 Klein: [B, C, H, W] -> [B, C*4, H//2, W//2]."""
    b, c, h, w = latents.shape
    latents = latents.view(b, c, h // 2, 2, w // 2, 2).permute(0, 1, 3, 5, 2, 4)
    return latents.reshape(b, c * 4, h // 2, w // 2)


def _pack_latents_flux2(latents: Any) -> Any:
    """Empacota latentes patchificados do FLUX.2 Klein para entrada no transformer: [B, C, H, W] -> [B, H*W, C]."""
    b, c, h, w = latents.shape
    return latents.reshape(b, c, h * w).permute(0, 2, 1)


def _prepare_latent_image_ids(
    batch_size: int, height: int, width: int, device: Any, dtype: Any
) -> Any:
    """Gera coordenadas de posição 2D para o Rotary Embedding (RoPE) de imagem do FLUX.1."""
    import torch

    h = height // 16
    w = width // 16
    latent_image_ids = torch.zeros(h, w, 3, device=device, dtype=dtype)
    latent_image_ids[..., 1] = latent_image_ids[..., 1] + torch.arange(h, device=device)[:, None]
    latent_image_ids[..., 2] = latent_image_ids[..., 2] + torch.arange(w, device=device)[None, :]
    latent_image_ids = latent_image_ids.reshape(h * w, 3)
    return latent_image_ids.repeat(batch_size, 1, 1)


def _prepare_text_ids(seq_len: int, device: Any, dtype: Any, batch_size: int = 1) -> Any:
    """Gera coordenadas 1D de posição para o Rotary Embedding (RoPE) textual do FLUX.1."""
    import torch

    txt_ids = torch.zeros(seq_len, 3, device=device, dtype=dtype)
    return txt_ids.repeat(batch_size, 1, 1)


def _prepare_flux2_latent_ids(latents: Any) -> Any:
    """Gera coordenadas de posição 4D (T, H, W, L) para o Rotary Embedding (RoPE) do FLUX.2 Klein."""
    import torch

    batch_size, _, height, width = latents.shape
    t = torch.arange(1, device=latents.device)
    h = torch.arange(height, device=latents.device)
    w = torch.arange(width, device=latents.device)
    l = torch.arange(1, device=latents.device)
    coords = torch.cartesian_prod(t, h, w, l)
    return coords.unsqueeze(0).expand(batch_size, -1, -1)


def _prepare_flux2_text_ids(prompt_embeds: Any) -> Any:
    """Gera coordenadas de posição 4D (T, H, W, L) para o Rotary Embedding (RoPE) textual do FLUX.2 Klein."""
    import torch

    batch_size, seq_len, _ = prompt_embeds.shape
    t = torch.arange(1, device=prompt_embeds.device)
    h = torch.arange(1, device=prompt_embeds.device)
    w = torch.arange(1, device=prompt_embeds.device)
    l = torch.arange(seq_len, device=prompt_embeds.device)
    coords = torch.cartesian_prod(t, h, w, l)
    return coords.unsqueeze(0).expand(batch_size, -1, -1)
