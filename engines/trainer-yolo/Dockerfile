# Engine trainer-yolo (ADR-0007 D5): mock treino (ENGINE_MOCK=1, default).
# O modo real @gpu (ultralytics) roda fora do compose — NÃO instale aqui.
# PyYAML é a única dependência extra (parse determinístico do config.yaml).
FROM python:3.12-slim@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea
RUN groupadd -g 1000 studio && useradd -u 1000 -g studio -s /bin/bash -m studio
RUN pip install --no-cache-dir pyyaml>=6.0 pillow>=10.0
WORKDIR /app
COPY engines/engine-kit/src/engine_kit/ /app/engine_kit/
COPY engines/trainer-yolo/src/trainer_yolo/ /app/trainer_yolo/
ENV ENGINE_MOCK=1 \
    MOCK_EPOCH_SLEEP_MS=200
USER studio
ENTRYPOINT ["python", "-m", "trainer_yolo"]
CMD ["train", "--config", "/dev/null", "--output", "/dev/null"]
