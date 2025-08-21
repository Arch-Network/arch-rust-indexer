#!/bin/bash

echo "Testing Blocks API Endpoint"
echo "============================"

echo -e "\n1. Testing basic blocks endpoint:"
curl -s "http://localhost:3001/api/blocks?limit=5&offset=0" | jq .

echo -e "\n2. Testing with filter_no_transactions=false:"
curl -s "http://localhost:3001/api/blocks?limit=5&offset=0&filter_no_transactions=false" | jq .

echo -e "\n3. Testing with filter_no_transactions=true:"
curl -s "http://localhost:3001/api/blocks?limit=5&offset=0&filter_no_transactions=true" | jq .

echo -e "\n4. Testing network stats:"
curl -s "http://localhost:3001/api/network/stats" | jq .

echo -e "\n5. Testing database directly:"
docker exec arch-indexer-microservices-postgres-1 psql -U postgres -d archindexer -c "SELECT COUNT(*) FROM blocks;"

echo -e "\n6. Testing database query (first 5 blocks):"
docker exec arch-indexer-microservices-postgres-1 psql -U postgres -d archindexer -c "SELECT height, hash, timestamp, bitcoin_block_height FROM blocks ORDER BY height DESC LIMIT 5;"

echo -e "\n7. Testing database query with JOIN (first 5 blocks):"
docker exec arch-indexer-microservices-postgres-1 psql -U postgres -d archindexer -c "SELECT b.height, b.hash, b.timestamp, b.bitcoin_block_height, COUNT(t.txid) as transaction_count FROM blocks b LEFT JOIN transactions t ON b.height = t.block_height GROUP BY b.height, b.hash, b.timestamp, b.bitcoin_block_height ORDER BY b.height DESC LIMIT 5;"

echo -e "\n8. Testing transactions count:"
docker exec arch-indexer-microservices-postgres-1 psql -U postgres -d archindexer -c "SELECT COUNT(*) FROM transactions;"
