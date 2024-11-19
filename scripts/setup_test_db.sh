#!/bin/bash

set -e

# Load test environment
set -a
source .env.test
set +a

# Check if PostgreSQL is running
pg_isready
if [ $? -ne 0 ]; then
    echo "PostgreSQL is not running. Please start PostgreSQL first."
    exit 1
fi

# Create test database if it doesn't exist
psql -h localhost -U postgres -tc "SELECT 1 FROM pg_database WHERE datname = 'arch_indexer_test'" | grep -q 1 || psql -h localhost -U postgres -c "CREATE DATABASE arch_indexer_test"

# Initialize schema with environment variables passed explicitly
DATABASE_URL=$DATABASE_URL cargo run --bin init_db

echo "Test database setup completed successfully"