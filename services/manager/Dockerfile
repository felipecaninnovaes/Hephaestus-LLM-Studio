FROM rust:1.97.1-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY services/api-principal/Cargo.toml ./services/api-principal/
COPY services/manager/Cargo.toml ./services/manager/
COPY services/orchestrator/Cargo.toml ./services/orchestrator/
RUN mkdir -p services/api-principal/src services/manager/src services/orchestrator/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && cargo build --release -p manager \
  && rm -rf services/*/src
COPY services/manager/src ./services/manager/src
# Workspace exige target em todos os membros: recria dummies (só manager compila via -p).
RUN mkdir -p services/api-principal/src services/orchestrator/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && touch services/manager/src/main.rs && cargo build --release -p manager

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/manager /app/manager
EXPOSE 8081
CMD ["/app/manager"]
