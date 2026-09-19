#!/bin/sh
# ==============================================================================
# infra/entrypoint-runpod-worker.sh — Entrypoint do nó worker na RunPod
# ==============================================================================
# O pod RunPod é ele próprio um container: não existe /var/run/docker.sock do
# host. Este entrypoint sobe um daemon Docker INTERNO (exige pod PRIVILEGED na
# RunPod) para que o orchestrator (EXEC_MODE=docker) consiga instanciar os
# trainers efêmeros com --gpus, exatamente como faz no compose/remote-node.yaml.
#
# O toolkit interno (nvidia-ctk, já configurado no build do Dockerfile) usa o
# driver que a RunPod monta dentro do pod — os containers-filho herdam a GPU.
# ==============================================================================
set -eu

mkdir -p /var/lib/docker /var/log

dockerd >/var/log/dockerd.log 2>&1 &

n=0
until docker info >/dev/null 2>&1; do
    n=$((n + 1))
    if [ "$n" -ge 60 ]; then
        echo "[runpod-worker] ERRO: dockerd interno não subiu em 60s. O pod RunPod está em modo PRIVILEGED? Últimas linhas do log:" >&2
        tail -n 40 /var/log/dockerd.log >&2 || true
        exit 1
    fi
    sleep 1
done

# Rede bridge dedicada aos trainers/daemon efêmeros no daemon interno.
# O template RunPod deve apontar ENGINE_NETWORK e DIFFUSION_DAEMON_NETWORK
# para "heph-engine" (DNS interno resolve "diffusion-daemon" entre containers).
docker network create heph-engine 2>/dev/null || true

echo "[runpod-worker] dockerd interno pronto — iniciando orchestrator"
exec /app/orchestrator
