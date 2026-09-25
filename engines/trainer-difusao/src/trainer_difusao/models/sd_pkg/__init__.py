"""
Subpacote de utilitários específicos de modelos Stable Diffusion (SD 1.5 e SDXL).
"""
from trainer_difusao.models.sd_pkg.embeddings import _compute_sdxl_embeddings
from trainer_difusao.models.sd_pkg.sample import (
    _generate_sample_sd15,
    _generate_sample_sdxl,
)

__all__ = [
    "_compute_sdxl_embeddings",
    "_generate_sample_sd15",
    "_generate_sample_sdxl",
]
