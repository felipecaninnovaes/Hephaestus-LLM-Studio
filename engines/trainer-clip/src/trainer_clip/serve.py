"""
Facade para o servidor de embeddings do trainer-clip (re-exporta de server e mock_embed).
"""
from __future__ import annotations

from trainer_clip.mock_embed import mock_vector
from trainer_clip.server import (
    DIM,
    MOCK,
    MODEL,
    PORT,
    Handler,
    run_clip_server,
)


def main() -> None:
    run_clip_server(port=PORT, model=MODEL, mock=MOCK)


if __name__ == "__main__":
    main()
