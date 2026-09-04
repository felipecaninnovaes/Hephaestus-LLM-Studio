FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/manager /app/manager
EXPOSE 8081
CMD ["/app/manager"]
