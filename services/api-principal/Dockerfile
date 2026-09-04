FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY services/api-principal/Cargo.toml ./services/api-principal/
RUN mkdir -p services/api-principal/src \
  && echo "fn main() {}" > services/api-principal/src/main.rs \
  && cargo build --release -p api-principal \
  && rm -rf services/api-principal/src
COPY services/api-principal/src ./services/api-principal/src
RUN touch services/api-principal/src/main.rs && cargo build --release -p api-principal

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/api-principal /app/api-principal
EXPOSE 8080
CMD ["/app/api-principal"]
