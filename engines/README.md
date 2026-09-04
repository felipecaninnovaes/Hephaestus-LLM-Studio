# Engines Python (uv)

Todo motor vive em `engines/<nome>` como projeto `uv` independente:

- `pyproject.toml` por engine, `requires-python >= 3.11` (pin em `.python-version` na raiz).
- Deps pesadas (torch, ultralytics, diffusers, open_clip) entram nos slices de treino, não no scaffold.
- Comandos: `uv sync --project engines/trainer-yolo`, `uv run --project engines/trainer-yolo -m trainer_yolo`.
- `uv.lock` por engine é commitado (reprodutibilidade, como o `Cargo.lock`).
