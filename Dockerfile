# Builder stage
FROM rustlang/rust:nightly-slim AS builder

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

# Build application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /usr/local/bin

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary and migrations
COPY --from=builder /usr/src/app/target/release/arch-indexer .
COPY --from=builder /usr/src/app/migrations ./migrations

ENV RUST_LOG=info

EXPOSE 8080

CMD ["arch-indexer"]