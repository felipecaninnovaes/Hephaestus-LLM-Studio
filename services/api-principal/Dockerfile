FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=build /app/target/release/api-principal /app/api-principal
EXPOSE 8080
CMD ["/app/api-principal"]
