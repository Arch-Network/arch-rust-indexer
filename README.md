# Arch Indexer

A high-performance indexing service built in Rust for archiving and querying data. This service utilizes PostgreSQL for storage, Redis for caching, and exposes metrics via Prometheus.

## Features

```
- RESTful API powered by Axum
- PostgreSQL database integration with SQLx
- Redis caching layer
- Prometheus metrics export
- Async runtime with Tokio
- Configuration via YAML
- Comprehensive error handling
- Thread-safe concurrent operations with DashMap
```

## Prerequisites

```
- Rust (latest stable version)
- PostgreSQL (13 or higher)
- Redis server
- Docker (optional, for containerized deployment)
```

## Installation

```
1. Clone the repository:

    git clone https://github.com/yourusername/arch-indexer.git
    cd arch-indexer

2. Copy the example configuration:

    cp config/config.example.yml config/config.yml

3. Set up your environment variables:

    cp .env.example .env
```

## Configuration

### Environment Variables

```
Create a .env file with the following variables:

DATABASE_URL=postgresql://username:password@localhost:5432/arch_indexer
REDIS_URL=redis://localhost:6379
RUST_LOG=info
```

### Configuration File

```
The config.yml file supports the following options:

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

```
Initialize the database schema:

cargo run --bin init_db
```

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
