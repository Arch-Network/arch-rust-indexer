# syntax=docker/dockerfile:1
FROM --platform=linux/amd64 rustlang/rust:nightly-slim AS builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
COPY .sqlx ./.sqlx

# Explicitly set the target platform
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
RUN rustup target add x86_64-unknown-linux-gnu
RUN cargo build --release --target x86_64-unknown-linux-gnu

# Runtime stage
FROM --platform=linux/amd64 debian:bookworm-slim

WORKDIR /usr/local/bin

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary and migrations from the correct target directory
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-gnu/release/arch-indexer .
COPY --from=builder /usr/src/app/migrations ./migrations

ENV RUST_LOG=info

EXPOSE 8080

CMD ["arch-indexer"]