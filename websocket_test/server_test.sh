#!/bin/bash

# Server-side WebSocket Testing Script
# Run this on the server to test WebSocket connectivity

echo "🔌 Server-side WebSocket Testing"
echo "================================"
echo ""

# Test endpoint
WEBSOCKET_URL="wss://rpc-beta.test.arch.network/ws"
echo "Testing: $WEBSOCKET_URL"
echo ""

# Function to check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Test 1: Check if port is reachable
echo "🧪 Test 1: Port Connectivity"
echo "-----------------------------"
if command_exists nc; then
    echo "Testing port 443 (HTTPS)..."
    nc -zv rpc-beta.test.arch.network 443
    echo ""
else
    echo "nc (netcat) not available"
fi

# Test 2: HTTP response
echo "🧪 Test 2: HTTP Response"
echo "------------------------"
if command_exists curl; then
    echo "Testing HTTP response..."
    curl -I "https://rpc-beta.test.arch.network/ws" 2>/dev/null | head -5
    echo ""
else
    echo "curl not available"
fi

# Test 3: WebSocket with websocat
echo "🧪 Test 3: WebSocket Connection (websocat)"
echo "-------------------------------------------"
if command_exists websocat; then
    echo "Testing WebSocket connection..."
    timeout 10s websocat "$WEBSOCKET_URL" 2>&1 | head -10
    echo ""
else
    echo "websocat not available - install with: cargo install websocat"
fi

# Test 4: WebSocket with wscat
echo "🧪 Test 4: WebSocket Connection (wscat)"
echo "----------------------------------------"
if command_exists wscat; then
    echo "Testing WebSocket connection..."
    timeout 10s wscat -c "$WEBSOCKET_URL" 2>&1 | head -10
    echo ""
else
    echo "wscat not available - install with: npm install -g wscat"
fi

# Test 5: Raw TCP connection
echo "🧪 Test 5: Raw TCP Connection"
echo "------------------------------"
if command_exists socat; then
    echo "Testing TCP connection to port 443..."
    timeout 5s socat - TCP:rpc-beta.test.arch.network:443 2>&1 | head -5
    echo ""
else
    echo "socat not available"
fi

# Test 6: Manual WebSocket handshake
echo "🧪 Test 6: Manual WebSocket Handshake"
echo "-------------------------------------"
if command_exists curl; then
    echo "Testing WebSocket upgrade..."
    curl -i -N -H "Connection: Upgrade" \
         -H "Upgrade: websocket" \
         -H "Sec-WebSocket-Version: 13" \
         -H "Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==" \
         "https://rpc-beta.test.arch.network/ws" 2>&1 | head -10
    echo ""
else
    echo "curl not available"
fi

echo "✅ All tests completed!"
echo ""
echo "📋 To test specific WebSocket methods:"
echo "1. Install websocat: cargo install websocat"
echo "2. Test connection: websocat $WEBSOCKET_URL"
echo "3. Send subscription: echo '{\"jsonrpc\":\"2.0\",\"method\":\"subscribe\",\"params\":[\"block\"],\"id\":1}' | websocat $WEBSOCKET_URL"
echo "4. Test getVersion: echo '{\"jsonrpc\":\"2.0\",\"method\":\"getVersion\",\"id\":1}' | websocat $WEBSOCKET_URL"
