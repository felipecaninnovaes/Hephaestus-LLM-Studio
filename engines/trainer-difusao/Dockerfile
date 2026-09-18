# Engine trainer-difusao (ADR-0018): mock treino (ENGINE_MOCK=1, default).
# O modo real @gpu (diffusers/peft) roda no container GPU dedicado.
# PyYAML e Pillow são as dependências mínimas para parse de config e datasets.
FROM python:3.12-slim@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea
RUN pip install --no-cache-dir "pyyaml>=6.0" "pillow>=10.0"
WORKDIR /app
COPY engines/engine-kit/src/engine_kit/ /app/engine_kit/
COPY engines/trainer-difusao/src/trainer_difusao/ /app/trainer_difusao/
ENV ENGINE_MOCK=1 \
    MOCK_EPOCH_SLEEP_MS=20
ENTRYPOINT ["python", "-m", "trainer_difusao"]
CMD ["train", "--config", "/dev/null", "--output", "/dev/null"]
