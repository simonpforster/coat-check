FROM rust:1.87-slim AS builder

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

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/coat-check /usr/local/bin/coat-check
ENV PORT=3000
EXPOSE 3000
CMD ["coat-check"]
