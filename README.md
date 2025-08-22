# 🚀 Arch Indexer

A high-performance blockchain indexing service built in Rust for the Arch Network, featuring both monolithic and microservices architectures. This service provides real-time blockchain data processing, RESTful APIs, and a modern web dashboard.

## 🏗️ Architecture Options

This project supports two deployment approaches:

### 1. **Monolithic Architecture** (Root Directory)
- **Single Rust binary** with integrated API server and indexer
- **Simpler deployment** and configuration
- **All-in-one solution** for smaller deployments
- **Port**: 8081 (configurable)

### 2. **Microservices Architecture** (`/arch-indexer-microservices`)
- **Separated services** for independent scaling
- **Frontend**: React/Next.js dashboard (Port 3000)
- **API Server**: Rust/Axum REST API (Port 3001)
- **Indexer**: Background blockchain processor
- **Better for production** and high-traffic scenarios

## 🎯 Features

- **Real-time blockchain indexing** with WebSocket support
- **RESTful API** powered by Axum
- **PostgreSQL database** integration with SQLx
- **Redis caching** layer for performance
- **Prometheus metrics** export
- **Async runtime** with Tokio
- **Configuration** via YAML
- **Comprehensive error handling**
- **Thread-safe concurrent operations** with DashMap
- **Modern web dashboard** with real-time updates

## 🚀 Quick Start

### Prerequisites
- **Rust** (latest stable version)
- **PostgreSQL** (13 or higher)
- **Redis** server
- **Docker** (optional, for containerized deployment)

### Option 1: Monolithic Deployment (Recommended for Development)

1. **Clone the repository:**
   ```bash
   git clone https://github.com/yourusername/arch-indexer.git
   cd arch-indexer
   ```

2. **Set up environment:**
   ```bash
   # Copy example config
   cp config.example.yml config.yml
   
   # Set environment variables
   export DB_PASSWORD=your_secure_password
   export ARCH_NODE_URL=http://your-arch-node:8081
   export ARCH_NODE_WEBSOCKET_URL=ws://your-arch-node:10081
   ```

3. **Start with Docker Compose:**
   ```bash
   docker-compose up -d
   ```

4. **Access the service:**
   - **API**: http://localhost:9090
   - **Database**: localhost:5432
   - **Redis**: localhost:6379

### Option 2: Microservices Deployment (Recommended for Production)

1. **Navigate to microservices directory:**
   ```bash
   cd arch-indexer-microservices
   ```

2. **Start all services:**
   ```bash
   docker-compose up -d
   ```

3. **Access services:**
   - **Frontend**: http://localhost:3000
   - **API**: http://localhost:3001
   - **Database**: localhost:5432
   - **Redis**: localhost:6379

## 🔧 Local Development

### Monolithic Development
```bash
# Install dependencies
cargo build

# Run the service
cargo run

# Run tests
cargo test

# Format code
cargo fmt

# Lint code
cargo clippy
```

### Microservices Development
```bash
# Frontend
cd arch-indexer-microservices/frontend
npm install
npm run dev

# API Server
cd arch-indexer-microservices/api-server
cargo run

# Indexer
cd arch-indexer-microservices/indexer
cargo run
```

## 📊 API Endpoints

### Core Endpoints
- `GET /` - Health check
- `GET /api/blocks` - List blocks with pagination
- `GET /api/blocks/{blockhash}` - Get block by hash
- `GET /api/blocks/height/{height}` - Get block by height
- `GET /api/transactions` - List transactions
- `GET /api/transactions/{txid}` - Get transaction by ID
- `GET /api/network-stats` - Network statistics
- `GET /api/sync-status` - Sync status
- `GET /metrics` - Prometheus metrics

### WebSocket Endpoints
- `ws://localhost:8081/ws` - Real-time blockchain updates

## 🗄️ Database Setup

### Initialize Schema
```bash
# Option 1: Using the binary
cargo run --bin init_schema

# Option 2: Using SQLx migrations
sqlx migrate run
```

### Database Configuration
```yaml
database:
  url: "postgresql://username:password@localhost:5432/archindexer"
  max_connections: 20
  min_connections: 5
  timeout_seconds: 30
```

## 🔐 Configuration

### Environment Variables
```bash
# Database
DATABASE_URL=postgresql://username:password@localhost:5432/archindexer
DB_PASSWORD=your_secure_password

# Arch Network
ARCH_NODE_URL=http://your-arch-node:8081
ARCH_NODE_WEBSOCKET_URL=ws://your-arch-node:10081

# Redis
REDIS_URL=redis://localhost:6379

# Application
RUST_LOG=info
APPLICATION__PORT=8081
APPLICATION__HOST=0.0.0.0
```

### Configuration File (`config.yml`)
```yaml
database:
  username: "postgres"
  password: "your_password"
  host: "localhost"
  port: 5432
  database_name: "archindexer"
  max_connections: 20
  min_connections: 5

application:
  host: "0.0.0.0"
  port: 8081

arch_node:
  url: "http://your-arch-node:8081"
  websocket_url: "ws://your-arch-node:10081"

indexer:
  batch_size: 100
  concurrent_batches: 5

websocket:
  enabled: true
  reconnect_interval_seconds: 5
  max_reconnect_attempts: 10
```

## 🐳 Docker Deployment

### Monolithic Deployment
```bash
# Build and run
docker-compose up -d

# View logs
docker-compose logs -f indexer

# Scale (if needed)
docker-compose up -d --scale indexer=2
```

### Microservices Deployment
```bash
# Start all services
cd arch-indexer-microservices
docker-compose up -d

# Scale individual services
docker-compose up -d --scale api-server=3 --scale frontend=2
```

## 📈 Monitoring & Observability

### Health Checks
- **Service health**: `GET /` endpoint
- **Database connectivity**: Built-in health checks
- **Redis connectivity**: Health check endpoints

### Metrics
- **Prometheus metrics**: `GET /metrics`
- **System metrics**: CPU, memory, disk usage
- **Application metrics**: Request latencies, sync status
- **Database metrics**: Connection pool stats

### Logging
- **Structured logging** with tracing
- **Configurable log levels** via `RUST_LOG`
- **Docker log aggregation** support

## 🚨 Troubleshooting

### Common Issues

1. **Database Connection Failed**
   ```bash
   # Check PostgreSQL status
   docker-compose logs postgres
   
   # Verify connection string
   echo $DATABASE_URL
   ```

2. **Indexer Not Syncing**
   ```bash
   # Check Arch Network connectivity
   curl $ARCH_NODE_URL/health
   
   # View indexer logs
   docker-compose logs -f indexer
   ```

3. **WebSocket Connection Issues**
   ```bash
   # Test WebSocket endpoint
   wscat -c ws://localhost:8081/ws
   ```

### Debug Mode
```bash
# Enable debug logging
RUST_LOG=debug docker-compose up indexer

# Check specific service logs
docker-compose logs -f api-server
```

## 🔄 Migration Between Architectures

### From Monolith to Microservices
1. **Stop monolith**: `docker-compose down`
2. **Start microservices**: `cd arch-indexer-microservices && docker-compose up -d`
3. **Update frontend config** to point to new API server
4. **Verify data consistency**

### From Microservices to Monolith
1. **Stop microservices**: `docker-compose down`
2. **Start monolith**: `cd .. && docker-compose up -d`
3. **Update frontend config** to point to monolith API
4. **Verify functionality**

## 🎯 Use Cases

### Choose Monolithic When:
- **Development/testing** environments
- **Small to medium** deployments
- **Simple infrastructure** requirements
- **Quick setup** needed

### Choose Microservices When:
- **Production** deployments
- **High traffic** scenarios
- **Independent scaling** needed
- **Team development** with different technologies

## 🤝 Contributing

1. **Fork** the repository
2. **Create** your feature branch (`git checkout -b feature/amazing-feature`)
3. **Commit** your changes (`git commit -m 'Add amazing feature'`)
4. **Push** to the branch (`git push origin feature/amazing-feature`)
5. **Open** a Pull Request

### Development Guidelines
- **Follow Rust conventions** and best practices
- **Write tests** for new functionality
- **Update documentation** for API changes
- **Use conventional commits** for commit messages

## 📚 Additional Resources

- **[Microservices README](./arch-indexer-microservices/README.md)** - Detailed microservices documentation
- **[Real-time Indexing Guide](./REALTIME_INDEXING.md)** - WebSocket and real-time sync details
- **[Deployment Guide](./deploy/README.md)** - Cloud deployment instructions
- **[API Documentation](./docs/api.md)** - Complete API reference

## 📄 License

[Your License Here]

---

**Happy Indexing! 🚀**

*Built with ❤️ using Rust, Axum, and modern web technologies*
