FROM rust:1.97.1-slim@sha256:8e8cf8f7fd54a2d23d5a743b3a03f56e26b6c774276c33fa0595111704ebb15c AS build
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
COPY packages/policies/vram-table.yaml ./packages/policies/vram-table.yaml
# Workspace exige target em todos os membros: recria dummies (só manager compila via -p).
RUN mkdir -p services/api-principal/src services/orchestrator/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && touch services/manager/src/main.rs && cargo build --release -p manager

FROM debian:trixie-slim@sha256:d7e12182ce18b85b93007c1dedf31f2d29e01ccf3182cc4017c709b6259bc132
WORKDIR /app
COPY --from=build /app/target/release/manager /app/manager
EXPOSE 8081
CMD ["/app/manager"]
