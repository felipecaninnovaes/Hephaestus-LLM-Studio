"""
Facade para o servidor daemon HTTP de inferência de difusão (re-exporta de serve_pkg).
"""
from __future__ import annotations

import sys
from trainer_difusao.serve_pkg import (
    DiffusionHandler,
    cmd_serve,
    _spec_key,
    _spec_matches,
    _make_spec,
    _gen_lock,
    _busy,
    _loaded_spec,
    _pipeline_cache,
    _server,
)

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

if __name__ == "__main__":
    cmd_serve(sys.argv[1:])
