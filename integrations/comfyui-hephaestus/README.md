# Hephaestus para ComfyUI

Custom node mínimo (só rotas HTTP, sem nós) que recebe LoRAs `.safetensors` enviados pelo Hephaestus LLM Studio, em partes de até 32 MiB, e os grava em `models/loras/hephaestus/`.

## Instalação

1. Copie esta pasta (`comfyui-hephaestus`) para `ComfyUI/custom_nodes/`.
2. Defina o token (o mesmo cadastrado no destino em *Configurações → ComfyUI*):
   - variável de ambiente `HEPHAESTUS_COMFY_TOKEN`, **ou**
   - arquivo `hephaestus_token.txt` (só o token) ao lado do `__init__.py`.
3. Reinicie o ComfyUI. Sem token configurado, todas as rotas respondem 503.

Teste: `curl -H "Authorization: Bearer <token>" http://HOST:8188/hephaestus/health`

Não precisa de dependências extras. Se o ComfyUI estiver atrás de proxy (ex.: RunPod), partes de 32 MiB cabem nos limites usuais.

## Testes

```
uv run --with aiohttp --with pytest --with pytest-aiohttp --with pytest-asyncio pytest
```
