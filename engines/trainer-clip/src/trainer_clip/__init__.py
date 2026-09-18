"""
Pacote de inferência e serviço de embeddings OpenCLIP.
"""
from trainer_clip.mock_embed import mock_vector
from trainer_clip.server import DIM, MODEL, PORT, MOCK, Handler, run_clip_server

__all__ = [
    "mock_vector",
    "DIM",
    "MODEL",
    "PORT",
    "MOCK",
    "Handler",
    "run_clip_server",
]
