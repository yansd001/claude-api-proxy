# ---- Stage 1: Build Vue frontend ----
FROM node:20-slim AS frontend-builder
WORKDIR /build
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

# ---- Stage 2: Build Rust backend ----
FROM rust:1.94-slim AS backend-builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY backend-rust/Cargo.toml backend-rust/Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
COPY backend-rust/src ./src
RUN cargo build --release

# ---- Stage 3: Runtime ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=backend-builder /build/target/release/claude-api-proxy .
COPY --from=frontend-builder /build/dist ./static

RUN mkdir -p /app/data
ENV CONFIG_PATH=/app/data/config.json
VOLUME /app/data

EXPOSE 8000

CMD ["./claude-api-proxy"]
