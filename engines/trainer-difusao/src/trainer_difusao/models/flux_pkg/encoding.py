"""
Codificação de prompts de texto usando Qwen3 para FLUX.2 Klein.
"""
from __future__ import annotations

from typing import Any


def _encode_qwen3_prompt(
    text_encoder: Any,
    tokenizer: Any,
    prompts: list[str],
    device: Any,
    dtype: Any,
    max_length: int = 512,
    hidden_states_layers: tuple[int, ...] = (9, 18, 27),
) -> Any:
    """Codifica prompts de texto usando o modelo Qwen3 para FLUX.2 Klein 4B, extraindo e concatenando camadas intermediárias."""
    import torch

    all_input_ids = []
    all_attention_masks = []
    for p in prompts:
        if hasattr(tokenizer, "apply_chat_template") and getattr(tokenizer, "chat_template", None):
            messages = [{"role": "user", "content": p}]
            try:
                text = tokenizer.apply_chat_template(
                    messages,
                    tokenize=False,
                    add_generation_prompt=True,
                    enable_thinking=False,
                )
            except Exception:
                text = tokenizer.apply_chat_template(
                    messages,
                    tokenize=False,
                    add_generation_prompt=True,
                )
        else:
            text = p
        inputs = tokenizer(
            text,
            return_tensors="pt",
            padding="max_length",
            truncation=True,
            max_length=max_length,
        )
        all_input_ids.append(inputs["input_ids"])
        all_attention_masks.append(inputs["attention_mask"])

    input_ids = torch.cat(all_input_ids, dim=0).to(device)
    attention_mask = torch.cat(all_attention_masks, dim=0).to(device)

    with torch.no_grad():
        output = text_encoder(
            input_ids=input_ids,
            attention_mask=attention_mask,
            output_hidden_states=True,
            use_cache=False,
        )
        num_layers = len(output.hidden_states)
        layers_to_use = [k for k in hidden_states_layers if k < num_layers]
        if not layers_to_use:
            layers_to_use = [num_layers - 1]

        out = torch.stack([output.hidden_states[k] for k in layers_to_use], dim=1)
        out = out.to(dtype=dtype, device=device)

        batch_size, num_channels, seq_len, hidden_dim = out.shape
        prompt_embeds = out.permute(0, 2, 1, 3).reshape(batch_size, seq_len, num_channels * hidden_dim)

    return prompt_embeds
