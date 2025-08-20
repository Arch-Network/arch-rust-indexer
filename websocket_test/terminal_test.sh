#!/bin/bash

# Terminal WebSocket Testing Script
# Run this on the machine to test WebSocket subscribe functionality

echo "🔌 Terminal WebSocket Subscribe Testing"
echo "======================================"
echo ""

# Test endpoint
WEBSOCKET_URL="ws://44.196.173.35:10081"
echo "Testing: $WEBSOCKET_URL"
echo ""

# Function to check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Test 1: Port connectivity
echo "🧪 Test 1: Port Connectivity"
echo "-----------------------------"
if command_exists nc; then
    echo "Testing port 10081..."
    nc -zv 44.196.173.35 10081
    echo ""
else
    echo "nc (netcat) not available - install with: sudo apt install netcat"
fi

# Test 2: WebSocket with websocat
echo "🧪 Test 2: WebSocket Subscribe (websocat)"
echo "------------------------------------------"
if command_exists websocat; then
    echo "Testing subscription with websocat..."
    echo ""
    echo "Sending subscription message..."
    echo '{"jsonrpc":"2.0","method":"subscribe","params":["blocks"],"id":1}' | websocat "$WEBSOCKET_URL" 2>&1 | head -10
    echo ""
else
    echo "websocat not available"
    echo "Install with: cargo install websocat"
    echo ""
fi

# Test 3: WebSocket with wscat
echo "🧪 Test 3: WebSocket Subscribe (wscat)"
echo "---------------------------------------"
if command_exists wscat; then
    echo "Testing subscription with wscat..."
    echo ""
    echo "Sending subscription message..."
    timeout 10s wscat -c "$WEBSOCKET_URL" 2>&1 | head -10
    echo ""
else
    echo "wscat not available"
    echo "Install with: sudo npm install -g wscat"
    echo ""
fi

# Test 4: Manual WebSocket testing
echo "🧪 Test 4: Manual WebSocket Testing"
echo "-----------------------------------"
echo "To test manually, run one of these commands:"
echo ""
echo "1. With websocat (interactive):"
echo "   websocat $WEBSOCKET_URL"
echo "   Then type: {\"method\":\"subscribe\",\"topic\":\"blocks\"}"
echo ""
echo "2. With wscat (interactive):"
echo "   wscat -c $WEBSOCKET_URL"
echo "   Then type: {\"method\":\"subscribe\",\"topic\":\"blocks\"}"
echo ""
echo "3. With netcat (raw TCP):"
echo "   nc 44.196.173.35 10081"
echo "   Then type: {\"method\":\"subscribe\",\"topic\":\"blocks\"}"
echo ""

# Test 5: Different message formats
echo "🧪 Test 5: Different Message Formats"
echo "------------------------------------"
echo "Try these different subscription formats:"
echo ""
echo "Format 1 (Correct Arch Format - should work!):"
echo '{"method":"subscribe","params":{"topic":"block","filter":{},"request_id":"test1"}}' | websocat "$WEBSOCKET_URL" 2>&1 | head -5
echo ""
echo "Format 2 (Transaction Topic):"
echo '{"method":"subscribe","params":{"topic":"transaction","filter":{},"request_id":"test2"}}' | websocat "$WEBSOCKET_URL" 2>&1 | head -5
echo ""
echo "Format 3 (Account Update):"
echo '{"method":"subscribe","params":{"topic":"account_update","filter":{},"request_id":"test3"}}' | websocat "$WEBSOCKET_URL" 2>&1 | head -5
echo ""
echo "Format 4 (With Filter):"
echo '{"method":"subscribe","params":{"topic":"block","filter":{"height":"latest"},"request_id":"test4"}}' | websocat "$WEBSOCKET_URL" 2>&1 | head -5
echo ""

echo "✅ All tests completed!"
echo ""
echo "📋 Next Steps:"
echo "1. Install websocat: cargo install websocat"
echo "2. Test interactively: websocat $WEBSOCKET_URL"
echo "3. Try different message formats to find what the server expects"
echo "4. Check server logs for any error messages"
