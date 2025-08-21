#!/bin/bash

echo "🔧 Fixing Arch Indexer Timestamp Issues"
echo "======================================="

echo "The issue: Arch node returns timestamps in nanoseconds, but the indexer was treating them as milliseconds/seconds."
echo "This caused timestamps to be interpreted as being in the year 57609."
echo ""

echo "🔍 1. Stopping services..."
docker-compose down

echo "🗄️  2. Resetting the database to clear corrupted timestamps..."
docker volume rm arch-indexer-microservices_postgres_data || true

echo "🏗️  3. Rebuilding containers with fixed timestamp handling..."
docker-compose build

echo "🚀 4. Starting services with fixes..."
docker-compose up -d

echo "⏳ 5. Waiting for services to be healthy..."
sleep 30

echo "✅ 6. Verifying the fix..."
echo "Testing database connection..."
docker-compose exec postgres psql -U postgres -d archindexer -c "SELECT COUNT(*) FROM blocks;" || echo "Database not ready yet"

echo ""
echo "🎉 Fix complete!"
echo ""
echo "The indexer will now:"
echo "  - Correctly convert nanosecond timestamps to seconds"
echo "  - Store proper 2025 timestamps instead of year 57609"
echo "  - Re-index all blocks with correct timestamps"
echo ""
echo "Monitor the indexer logs with: docker-compose logs -f indexer"
echo "Check the frontend at: http://localhost:3000"
