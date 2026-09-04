FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY services/orchestrator/Cargo.toml ./services/orchestrator/
RUN mkdir -p services/orchestrator/src \
  && echo "fn main() {}" > services/orchestrator/src/main.rs \
  && cargo build --release -p orchestrator \
  && rm -rf services/orchestrator/src
COPY services/orchestrator/src ./services/orchestrator/src
RUN touch services/orchestrator/src/main.rs && cargo build --release -p orchestrator

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/orchestrator /app/orchestrator
EXPOSE 8082
CMD ["/app/orchestrator"]
