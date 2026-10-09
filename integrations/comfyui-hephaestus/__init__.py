"""Custom node do Hephaestus: só rotas HTTP (sem nós). Veja README.md."""

from pathlib import Path

try:
    from .hephaestus_routes import build_routes, read_token
except ImportError:  # coleta do pytest importa este arquivo fora de um pacote
    from hephaestus_routes import build_routes, read_token

NODE_CLASS_MAPPINGS: dict = {}
NODE_DISPLAY_NAME_MAPPINGS: dict = {}
__all__ = ["NODE_CLASS_MAPPINGS", "NODE_DISPLAY_NAME_MAPPINGS"]


def _register() -> None:
    try:
        import folder_paths
        from server import PromptServer
    except ImportError:
        return  # fora do ComfyUI (ex.: testes): nada a registrar
    instance = getattr(PromptServer, "instance", None)
    if instance is None:
        return
    token_file = Path(__file__).with_name("hephaestus_token.txt")
    routes = build_routes(
        get_token=lambda: read_token(token_file),
        get_lora_dir=lambda: Path(folder_paths.get_folder_paths("loras")[0]),
    )
    # O ComfyUI registra instance.routes no app depois de carregar os custom nodes.
    for r in routes:
        instance.routes.route(r.method, r.path, **r.kwargs)(r.handler)


_register()
