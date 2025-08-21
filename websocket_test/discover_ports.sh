#!/bin/bash

# WebSocket Port Discovery Script
# This script helps find the correct WebSocket port for Arch Network nodes

echo "🔍 Arch Network WebSocket Port Discovery"
echo "========================================"
echo ""

# Common WebSocket ports to try
ports=(8081 9001 10081 8082 9002 10081 8080 9000 10080)

# Function to test a port
test_port() {
    local port=$1
    local host="rpc-beta.test.arch.network"
    
    echo "🧪 Testing port $port..."
    
    # Try to connect with timeout
    timeout 10s cargo run --quiet 2>/dev/null &
    local pid=$!
    
    # Wait a bit for connection attempt
    sleep 3
    
    # Check if process is still running (connection successful)
    if kill -0 $pid 2>/dev/null; then
        echo "✅ Port $port: Connection successful!"
        kill $pid 2>/dev/null
        return 0
    else
        echo "❌ Port $port: Connection failed or timed out"
        return 1
    fi
}

echo "📋 Testing common WebSocket ports for $host..."
echo ""

# Test each port
for port in "${ports[@]}"; do
    export WEBSOCKET_URL="wss://$host:$port"
    if test_port $port; then
        echo "🎯 Found working WebSocket port: $port"
        echo ""
        echo "🔧 To test this port manually:"
        echo "WEBSOCKET_URL=wss://$host:$port cargo run"
        echo ""
        break
    fi
    echo ""
done

echo "📚 For more information, see README.md"
echo "🔧 To test a specific port manually: WEBSOCKET_URL=wss://host:port cargo run"
