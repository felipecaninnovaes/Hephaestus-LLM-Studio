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
  && cargo build --release -p api-principal \
  && rm -rf services/*/src
COPY services/api-principal/src ./services/api-principal/src
# Workspace exige target em todos os membros: recria dummies (só api-principal compila via -p).
RUN mkdir -p services/manager/src services/orchestrator/src \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && touch services/api-principal/src/main.rs && cargo build --release -p api-principal

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/api-principal /app/api-principal
EXPOSE 8080
CMD ["/app/api-principal"]
