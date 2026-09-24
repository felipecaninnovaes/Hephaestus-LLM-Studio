"""
Cálculo de embeddings de texto para modelos Stable Diffusion (SDXL).
"""
from __future__ import annotations

from typing import Any

__all__ = ["_compute_sdxl_embeddings"]


def _compute_sdxl_embeddings(
    prompts: list[str],
    tokenizer_one: Any,
    tokenizer_two: Any,
    text_encoder_one: Any,
    text_encoder_two: Any,
    device: Any,
    target_dtype: Any = None,
) -> tuple[Any, Any]:
    import torch

    with torch.no_grad():
        tokens_one = tokenizer_one(
            prompts,
            padding="max_length",
            max_length=tokenizer_one.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).input_ids.to(device)
        enc_one = text_encoder_one(tokens_one, output_hidden_states=True)
        hidden_states_one = enc_one.hidden_states[-2]

        tokens_two = tokenizer_two(
            prompts,
            padding="max_length",
            max_length=tokenizer_two.model_max_length,
            truncation=True,
            return_tensors="pt",
        ).input_ids.to(device)
        enc_two = text_encoder_two(tokens_two, output_hidden_states=True)
        hidden_states_two = enc_two.hidden_states[-2]
        pooled_embeds = enc_two.text_embeds
        # Concatena canais de embedding (768 + 1280 = 2048)
        prompt_embeds = torch.concat([hidden_states_one, hidden_states_two], dim=-1)
        if target_dtype is not None:
            prompt_embeds = prompt_embeds.to(dtype=target_dtype)
            pooled_embeds = pooled_embeds.to(dtype=target_dtype)

    return prompt_embeds, pooled_embeds
