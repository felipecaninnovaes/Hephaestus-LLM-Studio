FROM rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS build

# Build-deps do spike F4.0: cmake, g++, pkg-config, perl (aws-lc-sys).
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake g++ pkg-config perl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY services/api-principal/Cargo.toml ./services/api-principal/
COPY services/manager/Cargo.toml ./services/manager/
COPY services/orchestrator/Cargo.toml ./services/orchestrator/
RUN mkdir -p services/api-principal/src services/manager/src services/orchestrator/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && cargo build --release -p orchestrator \
  && rm -rf services/*/src
COPY services/orchestrator/src ./services/orchestrator/src
# Workspace exige target em todos os membros: recria dummies (só orchestrator compila via -p).
RUN mkdir -p services/api-principal/src services/manager/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && touch services/orchestrator/src/main.rs && cargo build --release -p orchestrator

# Runtime: trixie-slim (GLIBC 2.41 — spike C2 provou; bookworm FALHA) + docker-cli.
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132
RUN apt-get update && apt-get install -y --no-install-recommends docker-cli \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/orchestrator /app/orchestrator
EXPOSE 8082
CMD ["/app/orchestrator"]
