#!/bin/bash

# Ubuntu WebSocket Testing Script
# Uses tools commonly available on Ubuntu systems

echo "🔌 Ubuntu WebSocket Testing"
echo "==========================="
echo ""

WEBSOCKET_URL="wss://rpc-beta.test.arch.network/ws"
echo "Testing: $WEBSOCKET_URL"
echo ""

# Test 1: Check if port is reachable
echo "🧪 Test 1: Port Connectivity"
echo "-----------------------------"
if command -v nc >/dev/null 2>&1; then
    echo "Testing port 443 (HTTPS)..."
    nc -zv rpc-beta.test.arch.network 443
    echo ""
else
    echo "nc (netcat) not available - install with: sudo apt install netcat"
fi

# Test 2: HTTP response
echo "🧪 Test 2: HTTP Response"
echo "------------------------"
if command -v curl >/dev/null 2>&1; then
    echo "Testing HTTP response..."
    curl -I "https://rpc-beta.test.arch.network/ws" 2>/dev/null | head -5
    echo ""
else
    echo "curl not available - install with: sudo apt install curl"
fi

# Test 3: WebSocket handshake with curl
echo "🧪 Test 3: WebSocket Handshake (curl)"
echo "--------------------------------------"
if command -v curl >/dev/null 2>&1; then
    echo "Testing WebSocket upgrade..."
    timeout 10s curl -i -N -H "Connection: Upgrade" \
         -H "Upgrade: websocket" \
         -H "Sec-WebSocket-Version: 13" \
         -H "Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==" \
         "https://rpc-beta.test.arch.network/ws" 2>&1 | head -10
    echo ""
else
    echo "curl not available"
fi

# Test 4: Try to install websocat
echo "🧪 Test 4: Install websocat"
echo "----------------------------"
echo "Attempting to install websocat..."

# Check if Rust is available
if command -v cargo >/dev/null 2>&1; then
    echo "Rust is available, installing websocat..."
    cargo install websocat
    if [ $? -eq 0 ]; then
        echo "✅ websocat installed successfully!"
        echo "Testing WebSocket connection..."
        timeout 10s ~/.cargo/bin/websocat "$WEBSOCKET_URL" 2>&1 | head -10
    else
        echo "❌ Failed to install websocat"
    fi
else
    echo "Rust not available. Installing Rust..."
    echo "Run this command to install Rust:"
    echo "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "Then: source ~/.cargo/env && cargo install websocat"
fi

echo ""

# Test 5: Try to install wscat
echo "🧪 Test 5: Install wscat (Node.js)"
echo "-----------------------------------"
if command -v npm >/dev/null 2>&1; then
    echo "npm is available, installing wscat..."
    sudo npm install -g wscat
    if [ $? -eq 0 ]; then
        echo "✅ wscat installed successfully!"
        echo "Testing WebSocket connection..."
        timeout 10s wscat -c "$WEBSOCKET_URL" 2>&1 | head -10
    else
        echo "❌ Failed to install wscat"
    fi
else
    echo "npm not available. Installing Node.js..."
    echo "Run these commands:"
    echo "curl -fsSL https://deb.nodesource.com/setup_lts.x | sudo -E bash -"
    echo "sudo apt-get install -y nodejs"
    echo "sudo npm install -g wscat"
fi

echo ""
echo "✅ All tests completed!"
echo ""
echo "📋 Manual WebSocket Testing Commands:"
echo ""
echo "1. With websocat (after installation):"
echo "   websocat $WEBSOCKET_URL"
echo "   echo '{\"jsonrpc\":\"2.0\",\"method\":\"subscribe\",\"params\":[\"block\"],\"id\":1}' | websocat $WEBSOCKET_URL"
echo ""
echo "2. With wscat (after installation):"
echo "   wscat -c $WEBSOCKET_URL"
echo ""
echo "3. With curl (basic handshake):"
echo "   curl -i -N -H \"Connection: Upgrade\" -H \"Upgrade: websocket\" -H \"Sec-WebSocket-Version: 13\" -H \"Sec-WebSocket-Key: x3JJHMbDL1EzLkh9GBhXDw==\" \"$WEBSOCKET_URL\""
