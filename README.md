# Arch Indexer

A high-performance indexing service built in Rust for archiving and querying data. This service utilizes PostgreSQL for storage, Redis for caching, and exposes metrics via Prometheus.

## Features
- RESTful API powered by Axum
- PostgreSQL database integration with SQLx
- Redis caching layer
- Prometheus metrics export
- Async runtime with Tokio
- Configuration via YAML
- Comprehensive error handling
- Thread-safe concurrent operations with DashMap

## Prerequisites
- Rust (latest stable version)
- PostgreSQL (13 or higher)
- Redis server
- Docker (optional, for containerized deployment)

## Local Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/arch-indexer.git
   cd arch-indexer
   ```

2. Create and configure your local database:
   ```bash
   # Create the database
   createdb archindexer

   # Initialize the schema
   cargo run --bin init_schema
   ```

3. Set up your environment variables in `.env`:
   ```bash
   DATABASE_URL=postgresql://postgres:postgres@localhost:5432/archindexer
   ARCH_NODE_URL=http://your-arch-node:9002
   REDIS_URL=redis://localhost:6379
   RUST_LOG=info
   ```

4. Start Redis (if not already running):
   ```bash
   # Using Docker
   docker run -d -p 6379:6379 redis:alpine
   # Or use your system's Redis service
   ```

5. Run the indexer in development mode:
   ```bash
   cargo run
   ```

   Or build and run in release mode:
   ```bash
   cargo build --release
   ./target/release/arch-indexer
   ```

6. Verify the service is running:
   ```bash
   curl http://localhost:8080/
   # Should return: {"message": "Arch Indexer API is running"}
   ```

## Docker Development Setup

If you prefer using Docker for local development:

1. Build and start all services:
   ```bash
   docker-compose up --build
   ```

2. The services will be available at:
   - Indexer API: http://localhost:8080
   - PostgreSQL: localhost:5432
   - Redis: localhost:6379

## Configuration

### Environment Variables
Required environment variables:
```bash
DATABASE_URL=postgresql://username:password@localhost:5432/archindexer
REDIS_URL=redis://localhost:6379
ARCH_NODE_URL=http://your-arch-node:9002
RUST_LOG=info
```

### Configuration File
The `config.yml` file supports the following options:
```yaml
server:
  host: "127.0.0.1"
  port: 8080

database:
  max_connections: 5
  timeout_seconds: 30

redis:
  ttl_seconds: 3600
  max_connections: 10

metrics:
  enabled: true
  port: 9090
```

## Database Setup

Initialize the database schema:

```bash
# Option 1: Using the binary
cargo run --bin init_schema

# Option 2: Using SQLx migrations
sqlx migrate run
```

Both methods will create the necessary database tables. The migrations method is preferred for production environments.

## Security Setup

Before running the indexer:

1. Copy the example environment file:
   ```bash
   cp .env.example .env
   ```

2. Copy the example configuration:
   ```bash
   cp config/config.example.yml config/config.yml
   ```

3. Update the `.env` and `config.yml` files with your secure values

⚠️ Never commit `.env` or `config.yml` files containing real credentials to the repository.

## Running the Service

```
### Development

cargo run --bin arch-indexer

### Production

cargo build --release
./target/release/arch-indexer
```

## API Endpoints

```
### Block Endpoints

GET /api/blocks
- Returns a list of the most recent blocks (limited to 200)
- Response includes block height, hash, timestamp, and Bitcoin block height

GET /api/blocks/{blockhash}
- Returns detailed information about a specific block by its hash
- Includes associated transactions for that block
- Returns 404 if block not found

GET /api/blocks/height/{height}
- Returns block information for a specific height
- Returns 404 if height not found

### Transaction Endpoints

GET /api/transactions
- Returns the most recent transactions (limited to 20)
- Ordered by block height in descending order
- Includes transaction ID, block height, status, and Bitcoin transaction IDs

GET /api/transactions/{txid}
- Returns detailed information about a specific transaction
- Includes full transaction data and status
- Returns 404 if transaction not found

### Network Statistics

GET /api/network-stats
- Returns current network statistics including:
  - Latest block height
  - Total transaction count
  - Network TPS (Transactions Per Second)
  - Time span calculations

### Sync Status

GET /api/sync-status
- Returns the current synchronization status:
  - Current block height
  - Target height
  - Sync progress
  - Whether sync is in progress

### Health Check

GET /
- Basic API health check endpoint
- Returns: {"message": "Arch Indexer API is running"}

### Metrics

GET /metrics
- Returns Prometheus-formatted metrics
- Includes system metrics, sync status, and performance indicators
```


## Monitoring

```
The service exports various metrics including:
- Request latencies
- Cache hit/miss ratios
- Database connection pool stats
- Index operation counters
```

## Error Handling

```
The service uses custom error types for better error handling and reporting. All errors are logged with appropriate context and returned as structured JSON responses.
```

## Development

```
### Running Tests

cargo test

### Code Formatting

cargo fmt

### Linting

cargo clippy
```

## Contributing

```
1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request
```
