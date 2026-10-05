# syntax=docker/dockerfile:1
# Base comum para engines GPU Hephaestus: PyTorch 2.6.0 + CUDA 12.4 + cuDNN 9 (runtime)
# Compartilha pacotes apt essenciais para processamento de imagem/OpenCV e criação do usuário studio (uid 1000).
FROM pytorch/pytorch:2.6.0-cuda12.4-cudnn9-runtime@sha256:77f17f843507062875ce8be2a6f76aa6aa3df7f9ef1e31d9d7432f4b0f563dee

# System libs required by opencv-python (cv2) e aceleração de imagem
# Base is Ubuntu (pytorch official); libgl1 pulls libxcb1/libx11 transitively.
RUN rm -f /etc/apt/apt.conf.d/docker-clean; \
    echo 'Binary::apt::APT::Keep-Downloaded-Packages "true";' > /etc/apt/apt.conf.d/keep-cache
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y --no-install-recommends \
      libgl1 libglib2.0-0 libsm6 libxext6 libxrender1

# Usuário studio (uid 1000 / gid 1000) compartilhado por todas as engines (PITFALLS:56-57)
RUN groupadd -g 1000 studio && useradd -u 1000 -g studio -s /bin/bash -m studio
