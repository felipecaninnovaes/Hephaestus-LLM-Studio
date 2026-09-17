"""Pos-passo de upscale x4 RRDBNet (torch-only, fatia flux2-motor-treino).

Arquitetura RRDBNet vendida neste arquivo (x4, 64 features, 23 blocos);
pesos via `hf_hub_download` por modelo (ver `UPSCALE_WEIGHTS`), com override
por env `REALESRGAN_WEIGHTS_4X` / `REALESRGAN_WEIGHTS_ULTRASHARP` /
`REALESRGAN_WEIGHTS_SIAX` (o generico `REALESRGAN_WEIGHTS` continua valendo
para "4x", back-compat). Pesos cacheados por (modelo, device) em variavel
global do modulo — upscale NAO entra em `pipeline_cache_key` nem na spec do
daemon (e pos-passo).

device: "cuda" se disponivel, senao cpu (caminho do dev).
Inference com tiling (tile 256, overlap 8) para limitar memoria.
model x4 gera 4x; se scale==2, resize final LANCZOS para 2x.

Qualquer falha (download/forward) -> _die honesto: o job falha, nunca
degrada silenciosamente.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

UPSCALE_MODELS = ("4x", "ultrasharp", "siax")
UPSCALE_SCALES = (2, 4)

UPSCALE_WEIGHTS: dict[str, tuple[str, str]] = {
    "4x": ("ai-forever/Real-ESRGAN", "RealESRGAN_x4.pth"),
    "ultrasharp": ("lokCX/4x-Ultrasharp", "4x-UltraSharp.pth"),
    "siax": ("gemasai/4x_NMKD-Siax_200k", "4x_NMKD-Siax_200k.pth"),
}

# Back-compat: aliases do entry "4x" (era o unico modelo suportado).
REALESRGAN_REPO = "ai-forever/Real-ESRGAN"
REALESRGAN_X4_FILE = "RealESRGAN_x4.pth"

# Env override por modelo (path local do .pth).
_WEIGHT_ENV_BY_MODEL = {
    "4x": "REALESRGAN_WEIGHTS_4X",
    "ultrasharp": "REALESRGAN_WEIGHTS_ULTRASHARP",
    "siax": "REALESRGAN_WEIGHTS_SIAX",
}
# Env generico legado: continua valendo para "4x" (back-compat).
_WEIGHT_ENV_LEGACY = "REALESRGAN_WEIGHTS"

_TILE = 256
_OVERLAP = 8

# Cache global dos pesos carregados: {(model, device_str): model}
_loaded_model: dict[tuple[str, str], Any] = {}


def _die(msg: str) -> None:
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


# ---------------------------------------------------------------------------
# RRDBNet (BasicSR/Real-ESRGAN: x4, num_feat=64, num_block=23, num_grow_ch=32)
# ---------------------------------------------------------------------------
def _build_rrdb_net() -> Any:
    import torch
    import torch.nn.functional as F
    from torch import nn

    def _default_init_weights(module_list, scale=0.1):
        for m in module_list:
            if isinstance(m, (nn.Conv2d, nn.Linear)):
                nn.init.kaiming_normal_(m.weight, a=0, mode="fan_in")
                m.weight.data *= scale
                if m.bias is not None:
                    nn.init.constant_(m.bias, 0)

    class ResidualDenseBlock(nn.Module):
        def __init__(self, num_feat=64, num_grow_ch=32):
            super().__init__()
            self.conv1 = nn.Conv2d(num_feat, num_grow_ch, 3, 1, 1)
            self.conv2 = nn.Conv2d(num_feat + num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv3 = nn.Conv2d(num_feat + 2 * num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv4 = nn.Conv2d(num_feat + 3 * num_grow_ch, num_grow_ch, 3, 1, 1)
            self.conv5 = nn.Conv2d(num_feat + 4 * num_grow_ch, num_feat, 3, 1, 1)
            self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)
            _default_init_weights(
                [self.conv1, self.conv2, self.conv3, self.conv4, self.conv5], 0.1
            )

        def forward(self, x):
            x1 = self.lrelu(self.conv1(x))
            x2 = self.lrelu(self.conv2(torch.cat((x, x1), 1)))
            x3 = self.lrelu(self.conv3(torch.cat((x, x1, x2), 1)))
            x4 = self.lrelu(self.conv4(torch.cat((x, x1, x2, x3), 1)))
            x5 = self.conv5(torch.cat((x, x1, x2, x3, x4), 1))
            return x5 * 0.2 + x

    class RRDB(nn.Module):
        def __init__(self, num_feat=64, num_grow_ch=32):
            super().__init__()
            self.rdb1 = ResidualDenseBlock(num_feat, num_grow_ch)
            self.rdb2 = ResidualDenseBlock(num_feat, num_grow_ch)
            self.rdb3 = ResidualDenseBlock(num_feat, num_grow_ch)

        def forward(self, x):
            out = self.rdb1(x)
            out = self.rdb2(out)
            out = self.rdb3(out)
            return out * 0.2 + x

    class RRDBNet(nn.Module):
        def __init__(
            self, num_in_ch=3, num_out_ch=3, num_feat=64, num_block=23, num_grow_ch=32
        ):
            super().__init__()
            self.conv_first = nn.Conv2d(num_in_ch, num_feat, 3, 1, 1)
            self.body = nn.Sequential(
                *[RRDB(num_feat, num_grow_ch) for _ in range(num_block)]
            )
            self.conv_body = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_up1 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_up2 = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_hr = nn.Conv2d(num_feat, num_feat, 3, 1, 1)
            self.conv_last = nn.Conv2d(num_feat, num_out_ch, 3, 1, 1)
            self.lrelu = nn.LeakyReLU(negative_slope=0.2, inplace=True)

        def forward(self, x):
            feat = self.conv_first(x)
            body_feat = self.conv_body(self.body(feat))
            feat = feat + body_feat
            feat = self.lrelu(
                self.conv_up1(F.interpolate(feat, scale_factor=2, mode="nearest"))
            )
            feat = self.lrelu(
                self.conv_up2(F.interpolate(feat, scale_factor=2, mode="nearest"))
            )
            out = self.conv_last(self.lrelu(self.conv_hr(feat)))
            return out

    return RRDBNet(num_in_ch=3, num_out_ch=3, num_feat=64, num_block=23)


def resolve_weights(model: str) -> str:
    """Resolve o path dos pesos do modelo de upscale (env override ou HF).

    Env por modelo (`_WEIGHT_ENV_BY_MODEL`) aponta para path local do .pth;
    o generico `REALESRGAN_WEIGHTS` continua valendo para "4x" (back-compat).
    Falhas -> _die citando o modelo e o env override.
    """
    if model not in UPSCALE_WEIGHTS:
        _die(f"Modelo de upscale inválido: {model}. Use: {', '.join(UPSCALE_MODELS)}.")
    for env_name in (_WEIGHT_ENV_BY_MODEL[model],) + (
        (_WEIGHT_ENV_LEGACY,) if model == "4x" else ()
    ):
        override = os.environ.get(env_name)
        if override and override.strip():
            p = override.strip()
            if not os.path.isfile(p):
                _die(f"{env_name} aponta para arquivo inexistente: {p} (modelo {model})")
            return p
    repo_id, filename = UPSCALE_WEIGHTS[model]
    try:
        from huggingface_hub import hf_hub_download
    except ImportError as e:
        _die(f"Upscale {model} exige 'huggingface-hub' instalado: {e}")
    try:
        return hf_hub_download(repo_id=repo_id, filename=filename)
    except Exception as e:  # noqa: BLE001 — rede/HF podem falhar de varios jeitos
        _die(
            f"Falha ao baixar pesos de upscale ({model}: {repo_id}/{filename}): "
            f"{e}. Defina {_WEIGHT_ENV_BY_MODEL[model]} com o path local do .pth."
        )


def _resolve_weights_path(model: str = "4x") -> str:
    return resolve_weights(model)


def _get_model(device: str, model: str = "4x") -> Any:
    key = (model, device)
    if key in _loaded_model:
        return _loaded_model[key]
    import torch

    net = _build_rrdb_net()
    weights_path = _resolve_weights_path(model)
    try:
        state = torch.load(weights_path, map_location="cpu", weights_only=True)
    except Exception as e:  # noqa: BLE001
        _die(f"Falha ao carregar pesos de upscale ({model}: {weights_path}): {e}")
    if isinstance(state, dict) and "params-ema" in state:
        state = state["params-ema"]
    elif isinstance(state, dict) and "params_ema" in state:
        state = state["params_ema"]
    elif isinstance(state, dict) and "params" in state:
        state = state["params"]
    try:
        net.load_state_dict(state, strict=True)
    except Exception as e:  # noqa: BLE001
        _die(
            f"Pesos de upscale incompativeis com RRDBNet x4 vendida ({model}: {weights_path}): {e}"
        )
    net.eval()
    net.to(device)
    _loaded_model[key] = net
    return net


def _upscale_tiled(model: Any, img_tensor: Any, device: str) -> Any:
    """Forward com tiling (tile 256, overlap 8) para limitar memoria."""
    import torch

    _, _, h, w = img_tensor.shape
    scale = 4
    tile = _TILE
    overlap = _OVERLAP
    out = torch.zeros((1, 3, h * scale, w * scale), dtype=torch.float32)
    with torch.inference_mode():
        for y in range(0, h, tile):
            for x in range(0, w, tile):
                y0 = max(0, y - overlap)
                x0 = max(0, x - overlap)
                y1 = min(h, y + tile + overlap)
                x1 = min(w, x + tile + overlap)
                tile_in = img_tensor[:, :, y0:y1, x0:x1].to(device)
                try:
                    tile_out = model(tile_in)
                except Exception as e:  # noqa: BLE001
                    _die(f"Falha no forward Real-ESRGAN (tile {x},{y}): {e}")
                tile_out = tile_out.detach().cpu()
                # Recorta o overlap na saida e cola na regiao correspondente.
                oy0 = (y - y0) * scale
                ox0 = (x - x0) * scale
                oy1 = oy0 + (min(h, y + tile) - y) * scale
                ox1 = ox0 + (min(w, x + tile) - x) * scale
                out[
                    :,
                    :,
                    y * scale : y * scale + (oy1 - oy0),
                    x * scale : x * scale + (ox1 - ox0),
                ] = tile_out[:, :, oy0:oy1, ox0:ox1]
    return out


def upscale_image(
    input_path: str | Path, output_path: str | Path, model: str = "4x", scale: int = 4
) -> dict[str, int]:
    """Aplica upscale RRDBNet x4 sobre o PNG salvo, re-salvando no mesmo arquivo.

    Retorna {"original_width","original_height","final_width","final_height"}.
    Falhas -> _die (job falha, nunca silencioso).
    """
    if model not in UPSCALE_MODELS:
        _die(f"Modelo de upscale inválido: {model}. Use: {', '.join(UPSCALE_MODELS)}.")
    if scale not in (2, 4):
        _die(f"Escala de upscale inválida: {scale}. Use 2 ou 4.")

    import torch
    from PIL import Image

    device = "cuda" if torch.cuda.is_available() else "cpu"
    realesr = _get_model(device, model)
    try:
        with Image.open(input_path) as _img:
            img = _img.convert("RGB")
    except Exception as e:  # noqa: BLE001
        _die(f"Falha ao abrir imagem para upscale ({input_path}): {e}")
    ow, oh = img.size

    import numpy as np

    arr = np.asarray(img).astype("float32") / 255.0
    tensor = torch.from_numpy(arr).permute(2, 0, 1).unsqueeze(0)
    up = _upscale_tiled(realesr, tensor, device)
    up = (
        (up.squeeze(0).permute(1, 2, 0).clamp(0, 1).numpy() * 255.0)
        .round()
        .astype("uint8")
    )
    out_img = Image.fromarray(up, mode="RGB")
    fw, fh = out_img.size  # == 4x
    if scale == 2:
        out_img = out_img.resize((ow * 2, oh * 2), Image.LANCZOS)
        fw, fh = out_img.size
    out_img.save(output_path, "PNG")
    return {
        "original_width": ow,
        "original_height": oh,
        "final_width": fw,
        "final_height": fh,
    }
