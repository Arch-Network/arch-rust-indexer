# WebSocket Connectivity Testing Guide

This guide explains how to test WebSocket connectivity on different types of Arch Network nodes.

## **Port Configuration**

**Important**: HTTP RPC and WebSocket use different ports!

- **HTTP RPC**: Usually port 8080 (for API calls)
- **WebSocket**: Usually port 8081 (for real-time events)
- **Beta Server**: May use different ports

## **Testing Options**

### **1. Local Validator (Recommended for Development)**

If you have a local validator running with WebSocket support:

```bash
# Start local validator with WebSocket enabled
cd ../../arch-network
cargo run --bin local-validator -- --websocket

# In another terminal, test WebSocket connection
WEBSOCKET_URL=ws://localhost:8081 cargo run
```

**Expected Results:**
- ✅ Connection successful
- ✅ `subscribe` method works
- ✅ Real-time events received
- ✅ Full WebSocket API available

### **2. Remote Validator with WebSocket**

If you have access to a remote validator that supports WebSocket:

```bash
WEBSOCKET_URL=ws://your-validator-ip:8081 cargo run
```

**Expected Results:**
- ✅ Connection successful
- ✅ `subscribe` method works
- ✅ Real-time events received

### **3. Beta Server (Limited Support)**

The beta server uses the `/ws` path for WebSocket connections:

```bash
# Beta server WebSocket endpoint
WEBSOCKET_URL=wss://rpc-beta.test.arch.network/ws cargo run
```

**Expected Results:**
- ✅ Connection successful (if correct endpoint)
- ❌ Most methods return "Method not found"
- ❌ No real-time event streaming
- ❌ Limited API support

### **4. Custom Endpoints**

Test against any custom WebSocket endpoint:

```bash
WEBSOCKET_URL=wss://your-endpoint.com:port cargo run
```

## **Finding the Correct WebSocket Port**

1. **Check validator documentation** for WebSocket port configuration
2. **Try common WebSocket ports**: 8081, 9001, 10081, 8082
3. **Use port scanning tools** to discover open ports
4. **Check firewall rules** - WebSocket ports may be different from HTTP ports

## **What to Look For**

### **✅ Successful WebSocket API:**
- Connection establishes without errors
- `subscribe` method works and returns success
- Real-time events are received
- Methods like `ping`, `getInfo` work

### **❌ Limited/No WebSocket API:**
- Connection may succeed but methods fail
- "Method not found" errors
- No real-time events
- Limited RPC method support

## **Troubleshooting**

### **Connection Issues:**
- **Connection refused**: Wrong port or server not running
- **502 Bad Gateway**: Server overloaded or down
- **404 Not Found**: Wrong WebSocket endpoint
- **SSL/TLS errors**: Use `wss://` for secure, `ws://` for insecure

### **Method Issues:**
- **"Method not found"**: Server doesn't support that method
- **"Invalid params"**: Wrong parameter format
- **"Internal error"**: Server-side issue

## **Setting Up Local Validator for Testing**

1. **Clone and build arch-network:**
```bash
cd ../../arch-network
cargo build --bin local-validator
```

2. **Start with WebSocket enabled:**
```bash
cargo run --bin local-validator -- --websocket
```

3. **Test WebSocket connection:**
```bash
cd ../rust/websocket_test
WEBSOCKET_URL=ws://localhost:8081 cargo run
```

## **Testing Different Methods**

The test tool automatically tries these methods:
- `subscribe` - Real-time event subscription
- `ping` - Basic connectivity test
- `getInfo` - Server information
- `getVersion` - Version information
- `getBlockHeight` - Current block height
- `getBlockCount` - Block count
- `getBestBlockHash` - Best block hash

## **Expected Output for Full WebSocket Support**

```
✅ WebSocket connection established successfully!
🔍 Trying method: subscribe
📤 Sending: {"method":"subscribe","params":{"topic":"block","filter":{},"request_id":"test1"}}
📨 Response 1: {"status":"Subscribed","subscription_id":"123","topic":"block","request_id":"test1"}
📨 Response 2: {"topic":"block","data":{"hash":"abc123","timestamp":1234567890}}}
```

## **Expected Output for Limited Support**

```
✅ WebSocket connection established successfully!
🔍 Trying method: subscribe
📤 Sending: {"method":"subscribe","params":{"topic":"block","filter":{},"request_id":"test1"}}
📨 Response 1: {"status":"Error","error":"Method not found"}
```

## **Next Steps**

1. **For Development**: Use local validator with WebSocket enabled
2. **For Production**: Use traditional polling until WebSocket support is available
3. **For Testing**: Use this tool to verify WebSocket capabilities of any endpoint
4. **Port Discovery**: Try different ports to find the correct WebSocket endpoint
