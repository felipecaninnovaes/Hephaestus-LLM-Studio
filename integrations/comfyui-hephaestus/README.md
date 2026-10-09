# Hephaestus para ComfyUI

Custom node mínimo (só rotas HTTP, sem nós) que recebe LoRAs `.safetensors` enviados pelo Hephaestus LLM Studio, em partes de até 32 MiB, e os grava em `models/loras/hephaestus/`.

## Instalação

1. Copie esta pasta (`comfyui-hephaestus`) para `ComfyUI/custom_nodes/`.
2. Gere um token forte (mínimo de 16 caracteres — o Hephaestus recusa tokens menores):

   ```
   python -c "import secrets;print(secrets.token_urlsafe(32))"
   ```

   Defina o token (o mesmo cadastrado no destino em *Configurações → ComfyUI*):
   - variável de ambiente `HEPHAESTUS_COMFY_TOKEN`, **ou**
   - arquivo `hephaestus_token.txt` (só o token) ao lado do `__init__.py`. Proteja o arquivo: `chmod 600 hephaestus_token.txt` (já está no `.gitignore`).
3. Reinicie o ComfyUI. Sem token configurado, todas as rotas respondem 503.

Teste: `curl -H "Authorization: Bearer <token>" http://HOST:8188/hephaestus/health`

Prefira HTTPS: o token trafega no cabeçalho `Authorization`. O proxy do RunPod já é HTTPS; em HTTP puro use só em rede confiável.

Limites: `size` máximo de 4 GiB (`size_too_large`), até 4 uploads ativos (`too_many_uploads`, 429) e espaço livre mínimo de `size` + 1 GiB no disco do ComfyUI (`insufficient_storage`, 507).

Não precisa de dependências extras. Se o ComfyUI estiver atrás de proxy (ex.: RunPod), partes de 32 MiB cabem nos limites usuais.

## Testes

```
uv run --with aiohttp --with pytest --with pytest-aiohttp --with pytest-asyncio pytest
```
