FROM rust:1.98-slim AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p coat-check-backoffice
COPY . .
RUN cargo build --release -p coat-check-backoffice

FROM debian:12.15-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app app
COPY --from=builder /app/target/release/coat-check-backoffice /usr/local/bin/coat-check-backoffice
USER app
ENV PORT=8080
EXPOSE 8080
CMD ["coat-check-backoffice"]
