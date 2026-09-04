FROM rust:1.82-slim AS build
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

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/orchestrator /app/orchestrator
EXPOSE 8082
CMD ["/app/orchestrator"]
