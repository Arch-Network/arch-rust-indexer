#!/bin/bash

# WebSocket Endpoint Testing Script
# This script helps test WebSocket connectivity to different Arch Network nodes

echo "🔌 Arch Network WebSocket Connectivity Tester"
echo "=============================================="
echo ""

# Function to test an endpoint
test_endpoint() {
    local url=$1
    local description=$2
    
    echo "🧪 Testing: $description"
    echo "📍 URL: $url"
    echo "⏱️  Running test for 30 seconds..."
    echo ""
    
    # Set the environment variable and run the test
    WEBSOCKET_URL="$url" timeout 35s cargo run --quiet 2>/dev/null
    
    echo ""
    echo "✅ Test completed for: $description"
    echo "----------------------------------------"
    echo ""
}

# Test different endpoints
echo "📋 Available Test Endpoints:"
echo "1. Local validator (ws://localhost:8081)"
echo "2. Beta server (wss://rpc-beta.test.arch.network/ws)"
echo "3. Custom endpoint"
echo ""

read -p "Choose an option (1-3) or press Enter to test all: " choice

case $choice in
    1)
        echo "Testing local validator..."
        test_endpoint "ws://localhost:8081" "Local Validator"
        ;;
    2)
        echo "Testing beta server..."
        test_endpoint "wss://rpc-beta.test.arch.network/ws" "Beta Server"
        ;;
    3)
        read -p "Enter custom WebSocket URL: " custom_url
        test_endpoint "$custom_url" "Custom Endpoint"
        ;;
    *)
        echo "Testing all endpoints..."
        echo ""
        
        # Test local validator
        test_endpoint "ws://localhost:8081" "Local Validator"
        
        # Test beta server
        test_endpoint "wss://rpc-beta.test.arch.network/ws" "Beta Server"
        
        echo "🎯 All tests completed!"
        ;;
esac

echo "📚 For more information, see README.md"
echo "🔧 To test a custom endpoint manually: WEBSOCKET_URL=your_url cargo run"
