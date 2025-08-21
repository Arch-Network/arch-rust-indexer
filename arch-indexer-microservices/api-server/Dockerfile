FROM rust:1.85-slim as builder

WORKDIR /app

# Install system dependencies for OpenSSL (sqlx)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

## Copy manifests and sources from the local build context
COPY Cargo.toml ./Cargo.toml
COPY src ./src
COPY .sqlx ./.sqlx

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
COPY --from=builder /app/target/release/api-server /app/api-server

# Create non-root user
RUN useradd -r -s /bin/false app && chown -R app:app /app
USER app

# Expose port
EXPOSE 3001

# Run the binary
CMD ["./api-server"]
