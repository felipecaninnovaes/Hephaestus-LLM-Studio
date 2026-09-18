"""
Pacote do servidor daemon HTTP de inferência quente para difusão.
"""
from trainer_difusao.serve_pkg.state import (
    _spec_key,
    _spec_matches,
    _make_spec,
    _gen_lock,
    _busy,
    _loaded_spec,
    _pipeline_cache,
    _server,
)
from trainer_difusao.serve_pkg.handler import DiffusionHandler
from trainer_difusao.serve_pkg.server import cmd_serve

__all__ = [
    "DiffusionHandler",
    "cmd_serve",
    "_spec_key",
    "_spec_matches",
    "_make_spec",
    "_gen_lock",
    "_busy",
    "_loaded_spec",
    "_pipeline_cache",
    "_server",
]
