FROM rust:1.98-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/api/Cargo.toml crates/api/Cargo.toml
COPY crates/web/Cargo.toml crates/web/Cargo.toml

# Build dependencies first (cached layer)
RUN mkdir -p crates/api/src && echo "fn main() {}" > crates/api/src/main.rs
RUN mkdir -p crates/web/src && echo "fn main() {}" > crates/web/src/main.rs
RUN cargo build --release -p coat-check-web
RUN rm crates/web/src/main.rs

# Build actual source
COPY crates/web/src crates/web/src
COPY crates/web/templates crates/web/templates
RUN touch crates/web/src/main.rs && cargo build --release -p coat-check-web

FROM debian:12.15-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd -r app && useradd -r -g app app
COPY --from=builder /app/target/release/coat-check-web /usr/local/bin/coat-check-web
USER app
ENV PORT=3000
EXPOSE 3000
CMD ["coat-check-web"]
