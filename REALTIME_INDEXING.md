# 🚀 Real-Time Indexing System

## Overview

The Arch Indexer now supports **real-time indexing** via WebSocket connections, providing instant access to blockchain events as they occur. This system combines real-time WebSocket streaming with traditional polling sync for maximum reliability and performance.

## ✨ Features

### 🔌 Real-Time WebSocket Support
- **Live Block Streaming**: Receive new blocks instantly as they're mined
- **Transaction Events**: Real-time transaction processing and status updates
- **Account Updates**: Track account changes in real-time
- **Rollback/Reapply Events**: Handle transaction rollbacks and reapplications
- **DKG Events**: Monitor Distributed Key Generation events

### 🏗️ Hybrid Sync Architecture
- **WebSocket Primary**: Real-time event processing for live data
- **Polling Fallback**: Traditional sync for catching up and handling gaps
- **Automatic Failover**: Seamless switching between sync modes
- **Gap Detection**: Automatic detection and filling of missing blocks

### 📊 Enhanced API Endpoints
- **Real-time Status**: `/api/realtime/status`
- **Recent Events**: `/api/realtime/events`
- **WebSocket Stats**: `/api/websocket/stats`
- **Live Data**: All existing endpoints now include real-time data

## 🎯 Configuration

### WebSocket Settings
```yaml
websocket:
  enabled: true
  reconnect_interval_seconds: 5
  max_reconnect_attempts: 10

arch_node:
  websocket_url: "ws://44.196.173.35:10081"  # Working WebSocket endpoint

indexer:
  enable_realtime: true
```

### Environment Variables
```bash
# Enable real-time indexing
ENABLE_REALTIME=true

# WebSocket endpoint
WEBSOCKET_URL=ws://44.196.173.35:10081

# Logging level
RUST_LOG=info
```

## 🏃‍♂️ Getting Started

### 1. Enable Real-Time Indexing
```bash
# Set environment variables
export ENABLE_REALTIME=true
export WEBSOCKET_URL=ws://44.196.173.35:10081

# Or update config.yml
```

### 2. Start the Indexer
```bash
cargo run
```

### 3. Monitor Real-Time Status
```bash
# Check real-time status
curl http://localhost:3000/api/realtime/status

# View WebSocket statistics
curl http://localhost:3000/api/websocket/stats

# Get recent events
curl http://localhost:3000/api/realtime/events
```

## 🔧 Architecture

### WebSocket Client (`WebSocketClient`)
- **Connection Management**: Automatic connection and reconnection
- **Topic Subscription**: Subscribes to all available event types
- **Event Parsing**: Parses incoming WebSocket messages
- **Error Handling**: Robust error handling and recovery

### Real-Time Processor (`RealtimeProcessor`)
- **Event Processing**: Handles all WebSocket event types
- **Database Storage**: Stores events in PostgreSQL
- **Deduplication**: Prevents duplicate event processing
- **Batch Processing**: Efficient batch database operations

### Hybrid Sync Manager (`HybridSync`)
- **Coordination**: Manages both real-time and traditional sync
- **State Management**: Tracks sync status and progress
- **Failover Logic**: Automatic fallback to polling sync
- **Performance Optimization**: Adjusts sync intervals based on real-time status

## 📡 Event Types

### Block Events
```json
{
  "topic": "block",
  "data": {
    "hash": "block_hash_here",
    "timestamp": 1755702969868190
  }
}
```

### Transaction Events
```json
{
  "topic": "transaction",
  "data": {
    "hash": "tx_hash_here",
    "status": "confirmed",
    "program_ids": ["program1", "program2"]
  }
}
```

### Account Update Events
```json
{
  "topic": "account_update",
  "data": {
    "account": "account_address",
    "transaction_hash": "tx_hash_here"
  }
}
```

### Rollback/Reapply Events
```json
{
  "topic": "rolledback_transactions",
  "data": {
    "transaction_hashes": ["tx1", "tx2", "tx3"]
  }
}
```

### DKG Events
```json
{
  "topic": "dkg",
  "data": {
    "status": "active"
  }
}
```

## 🚦 API Endpoints

### Real-Time Status
```bash
GET /api/realtime/status
```
Returns current real-time indexing status including:
- WebSocket connection status
- Last block received
- Events per second
- Active subscriptions

### Recent Events
```bash
GET /api/realtime/events
```
Returns recent real-time events with:
- Event type and data
- Timestamps
- Total event count
- Last update time

### WebSocket Statistics
```bash
GET /api/websocket/stats
```
Returns WebSocket connection statistics:
- Connection status
- Endpoint information
- Uptime and message counts
- Subscription topics

## 🔍 Monitoring & Debugging

### Logs
```bash
# Enable detailed logging
RUST_LOG=debug cargo run

# Monitor specific components
RUST_LOG=indexer::realtime_processor=debug cargo run
```

### Metrics
```bash
# View Prometheus metrics
curl http://localhost:3000/metrics
```

### Health Checks
```bash
# Check API health
curl http://localhost:3000/

# Check sync status
curl http://localhost:3000/api/sync-status
```

## 🚨 Troubleshooting

### WebSocket Connection Issues
1. **Check endpoint**: Verify `websocket_url` in config
2. **Network connectivity**: Ensure firewall allows WebSocket connections
3. **Server status**: Verify WebSocket server is running
4. **Reconnection**: Check reconnection settings

### Event Processing Issues
1. **Database connectivity**: Verify PostgreSQL connection
2. **Event format**: Check event parsing logic
3. **Memory usage**: Monitor for memory leaks
4. **Error logs**: Review error messages in logs

### Performance Issues
1. **Batch size**: Adjust database batch processing
2. **Connection pooling**: Optimize database connection settings
3. **Indexing**: Ensure proper database indexes
4. **Resource limits**: Check system resource usage

## 🔮 Future Enhancements

### Planned Features
- **WebSocket Server**: Serve real-time data to clients
- **Event Filtering**: Client-side event filtering
- **Streaming API**: Server-sent events for real-time updates
- **Event Persistence**: Long-term event storage and retrieval
- **Analytics**: Real-time analytics and metrics

### Performance Optimizations
- **Event Batching**: Improved batch processing
- **Caching**: Redis-based event caching
- **Compression**: WebSocket message compression
- **Load Balancing**: Multiple WebSocket connections

## 📚 References

- [Arch Network WebSocket Protocol](https://github.com/arch-network/arch-network)
- [Tokio WebSocket](https://docs.rs/tokio-tungstenite)
- [SQLx Async Database](https://docs.rs/sqlx)
- [Axum Web Framework](https://docs.rs/axum)

## 🤝 Contributing

To contribute to the real-time indexing system:

1. **Fork the repository**
2. **Create a feature branch**
3. **Implement your changes**
4. **Add tests and documentation**
5. **Submit a pull request**

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
