FROM rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/heph-contracts ./crates/heph-contracts/
COPY services/api-principal/Cargo.toml ./services/api-principal/
COPY services/manager/Cargo.toml ./services/manager/
COPY services/orchestrator/Cargo.toml ./services/orchestrator/
# migrate! roda em build-time: migrations ANTES do primeiro cargo build.
COPY services/api-principal/migrations ./services/api-principal/migrations/
RUN mkdir -p services/api-principal/src services/manager/src services/orchestrator/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && cargo build --release -p api-principal \
  && rm -rf services/*/src
# O rm acima apaga só */src: migrations/ segue intacta p/ a fase real (migrate! embutido).
COPY services/api-principal/src ./services/api-principal/src
# Workspace exige target em todos os membros: recria dummies (só api-principal compila via -p).
RUN mkdir -p services/manager/src services/orchestrator/src \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && touch services/api-principal/src/main.rs && cargo build --release -p api-principal

# Runtime = MESMA base do builder (trixie, glibc 2.41): aws-lc-sys puxa
# símbolos GLIBC_2.38+ e quebra no bookworm (2.36) — lição 2026-09-05,
# risco R10 da ADR-0003 materializado por tag mutável do rust:slim.
FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132
RUN groupadd -g 1000 studio && useradd -u 1000 -g studio -s /bin/bash -m studio
COPY --from=build --chown=studio:studio /app/target/release/api-principal /app/api-principal
USER studio
EXPOSE 8080
CMD ["/app/api-principal"]
