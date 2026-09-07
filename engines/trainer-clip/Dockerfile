# Embedder (ADR-0004 D1, 3f.3): stdlib puro no modo mock (ENGINE_MOCK=1 default
# do compose). O modo real CLIP (torch/open_clip) roda fora do compose (@gpu
# manual, extras [serve] do pyproject) — NÃO instale nada aqui.
FROM python:3.12-slim@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea
WORKDIR /app
COPY src/trainer_clip/serve.py /app/serve.py
ENV ENGINE_MOCK=1 \
    MODEL=ViT-B-32 \
    PORT=8090
EXPOSE 8090
CMD ["python", "/app/serve.py"]
