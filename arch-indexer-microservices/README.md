# 🚀 Arch Indexer - Microservices Architecture

A modern, scalable blockchain indexer built with microservices architecture, featuring real-time blockchain data processing and a beautiful React dashboard.

## 🏗️ Architecture Overview

This project has been refactored from a monolithic structure into a clean microservices architecture:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Frontend      │    │   API Server    │    │   Background    │
│   (React/Next)  │◄──►│   (Rust/Axum)   │◄──►│   Indexer      │
│   Port: 3000    │    │   Port: 3001    │    │   (Rust)       │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                                │                       │
                                ▼                       ▼
                       ┌─────────────────┐    ┌─────────────────┐
                       │   PostgreSQL    │    │   Arch Network  │
                       │   Database      │    │   RPC/WebSocket │
                       └─────────────────┘    └─────────────────┘
```

## 🎯 Services

### 1. **Frontend Service** (`/frontend`)
- **Technology**: React + Next.js + TypeScript
- **Port**: 3000
- **Features**: 
  - Real-time blockchain dashboard
  - Sync progress visualization
  - Block and transaction browsing
  - Search functionality
  - Responsive design

### 2. **API Server Service** (`/api-server`)
- **Technology**: Rust + Axum
- **Port**: 3001
- **Features**:
  - REST API endpoints
  - WebSocket server for real-time updates
  - Read-only database access
  - CORS support
  - Health checks

### 3. **Background Indexer Service** (`/indexer`)
- **Technology**: Rust + Tokio
- **Features**:
  - Blockchain data processing
  - Real-time WebSocket sync
  - Bulk historical sync
  - Database persistence
  - No HTTP server (background only)

### 4. **Database Service**
- **Technology**: PostgreSQL 15
- **Port**: 5432
- **Features**:
  - Persistent blockchain data
  - Optimized indexes
  - Connection pooling

### 5. **Cache Service** (Optional)
- **Technology**: Redis 7
- **Port**: 6379
- **Features**:
  - Block cache
  - Rate limiting
  - Session storage

## 🚀 Quick Start

### Prerequisites
- Docker & Docker Compose
- Rust toolchain (for local development)
- Node.js 18+ (for frontend development)

### 1. Clone and Setup
```bash
git clone <your-repo>
cd arch-indexer-microservices
```

### 2. Configuration
Copy your existing `config.yml` to the root directory, or create a new one:
```yaml
database:
  url: "postgresql://postgres:postgres@postgres:5432/archindexer"

arch_node:
  url: "http://your-arch-node:8080"
  websocket_url: "ws://your-arch-node:8080"

application:
  host: "0.0.0.0"
  port: 3001

indexer:
  enable_realtime: true
  bulk_sync_mode: true

websocket:
  enabled: true
```

### 3. Start All Services
```bash
docker-compose up -d
```

### 4. Access Services
- **Frontend**: http://localhost:3000
- **API**: http://localhost:3001
- **Database**: localhost:5432
- **Redis**: localhost:6379

## 🔧 Development

### Frontend Development
```bash
cd frontend
npm install
npm run dev
```

### API Server Development
```bash
cd api-server
cargo build
cargo run
```

### Indexer Development
```bash
cd indexer
cargo build
cargo run
```

## 📊 API Endpoints

### REST API
- `GET /api/network/stats` - Network statistics
- `GET /api/blocks` - List blocks with pagination
- `GET /api/transactions` - List transactions with pagination
- `GET /api/search?q=<query>` - Search blockchain
- `GET /health` - Health check

### WebSocket
- `ws://localhost:3001/ws` - Real-time updates

## 🐳 Docker Deployment

### Production Build
```bash
# Build all services
docker-compose -f docker-compose.yml -f docker-compose.prod.yml up -d

# Scale services
docker-compose up -d --scale api-server=3 --scale frontend=2
```

### Environment Variables
```bash
# Database
DATABASE_URL=postgresql://user:pass@host:5432/db

# Arch Network
ARCH_NODE_URL=http://your-node:8080
ARCH_NODE_WEBSOCKET_URL=ws://your-node:8080

# Features
ENABLE_REALTIME=true
WEBSOCKET_ENABLED=true
```

## 📈 Scaling

### Horizontal Scaling
- **API Server**: Scale based on user traffic
- **Frontend**: Scale based on user load
- **Indexer**: Usually single instance (stateful)

### Load Balancing
```bash
# Scale API servers
docker-compose up -d --scale api-server=3

# Use Nginx for load balancing
docker-compose up -d nginx
```

## 🔍 Monitoring

### Health Checks
- All services include health check endpoints
- Docker health checks for container monitoring
- Database connection monitoring

### Logging
- Structured logging with tracing
- Docker log aggregation
- Centralized log management (optional)

## 🚨 Troubleshooting

### Common Issues

1. **Database Connection Failed**
   ```bash
   docker-compose logs postgres
   docker-compose logs api-server
   ```

2. **Indexer Not Syncing**
   ```bash
   docker-compose logs indexer
   # Check Arch Network connectivity
   ```

3. **Frontend Can't Connect to API**
   ```bash
   # Check API server health
   curl http://localhost:3001/health
   
   # Check Next.js proxy configuration
   ```

### Debug Mode
```bash
# Run with debug logging
RUST_LOG=debug docker-compose up indexer
```

## 🔄 Migration from Monolith

### What Changed
- ✅ **Separated concerns**: Indexing vs API vs UI
- ✅ **Independent scaling**: Scale services based on load
- ✅ **Technology choice**: React for frontend, Rust for backend
- ✅ **Containerization**: Easy deployment and scaling
- ✅ **Health monitoring**: Built-in health checks

### What Stayed the Same
- ✅ **Database schema**: No changes needed
- ✅ **API contracts**: Same endpoints and responses
- ✅ **Core logic**: Indexing algorithms unchanged
- ✅ **Configuration**: Same config format

## 🎉 Benefits

1. **Scalability**: Scale UI and API independently
2. **Maintainability**: Clear separation of concerns
3. **Development**: Frontend hot-reload without affecting indexer
4. **Deployment**: Deploy updates without downtime
5. **Monitoring**: Independent health checks and metrics
6. **Technology**: Use best tools for each job

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Test with Docker Compose
5. Submit a pull request

## 📝 License

[Your License Here]

---

**Happy Indexing! 🚀**
