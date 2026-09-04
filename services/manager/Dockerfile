FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY services/manager/Cargo.toml ./services/manager/
RUN mkdir -p services/manager/src \
  && echo "fn main() {}" > services/manager/src/main.rs \
  && cargo build --release -p manager \
  && rm -rf services/manager/src
COPY services/manager/src ./services/manager/src
RUN touch services/manager/src/main.rs && cargo build --release -p manager

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/manager /app/manager
EXPOSE 8081
CMD ["/app/manager"]
