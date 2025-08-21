FROM rustlang/rust:nightly-slim as builder

WORKDIR /app

# Install build dependencies including OpenSSL
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build the application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/indexer /app/indexer

# Copy configuration
COPY config.yml /app/config.yml

# Create non-root user
RUN useradd -r -s /bin/false app && chown -R app:app /app
USER app

# Expose port (though indexer doesn't serve HTTP)
EXPOSE 8080

# Health check - check if the indexer binary is running
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD ls -la /proc/*/exe 2>/dev/null | grep -q "/app/indexer" || exit 1

# Run the binary
CMD ["./indexer"]
