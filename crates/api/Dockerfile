FROM rust:1.98-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/api/Cargo.toml crates/api/Cargo.toml

# Build dependencies first (cached layer)
RUN mkdir -p crates/api/src && echo "fn main() {}" > crates/api/src/main.rs
RUN cargo build --release -p coat-check
RUN rm crates/api/src/main.rs

# Build actual source
COPY crates/api/src crates/api/src
RUN touch crates/api/src/main.rs && cargo build --release -p coat-check

FROM debian:12.15-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app app
COPY --from=builder /app/target/release/coat-check /usr/local/bin/coat-check
USER app
ENV PORT=8080
EXPOSE 8080
CMD ["coat-check"]
