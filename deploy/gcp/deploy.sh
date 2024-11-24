#!/bin/bash
set -e

# Store the root directory of the project
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Check required environment variables
if [ -z "$PROJECT_ID" ] || [ -z "$DB_USER" ] || [ -z "$DB_PASSWORD" ]; then
    echo "Error: Required environment variables not set"
    echo "Please set: PROJECT_ID, DB_USER, DB_PASSWORD"
    exit 1
fi

# Retrieve the instance connection name from Terraform outputs
cd "$ROOT_DIR/deploy/gcp/terraform"
DB_INSTANCE=$(terraform output -raw instance_connection_name)
if [ -z "$DB_INSTANCE" ]; then
    echo "Error: DB_INSTANCE is not set"
    exit 1
fi
echo "DB_INSTANCE: $DB_INSTANCE"

# Change back to directory
cd "$ROOT_DIR"

# Build and push Docker image first
export DOCKER_BUILDKIT=1
echo "Building and pushing Docker image..."
docker build --platform linux/amd64 -t gcr.io/$PROJECT_ID/arch-rust-indexer:latest .
docker push gcr.io/$PROJECT_ID/arch-rust-indexer:latest

# Create terraform.tfvars
cat > terraform.tfvars << EOF
project_id    = "$PROJECT_ID"
region        = "us-central1"
db_username   = "$DB_USER"
db_password   = "$DB_PASSWORD"
arch_node_url = "${ARCH_NODE_URL:-http://leader:9002}"
redis_url     = "${REDIS_URL:-redis://localhost:6379}"
EOF

# Start Cloud SQL Proxy for local database access
echo "Starting Cloud SQL Proxy..."
cloud-sql-proxy "$DB_INSTANCE" --port 5433 &
PROXY_PID=$!
sleep 5

# Run migrations first
echo "Running database migrations..."
cd "$ROOT_DIR"  # Return to root directory for migrations
DATABASE_URL="postgresql://$DB_USER:$DB_PASSWORD@localhost:5433/archindexer" \
sqlx migrate run

# Generate SQLx prepare files
echo "Generating SQLx prepare files..."
DATABASE_URL="postgresql://$DB_USER:$DB_PASSWORD@localhost:5433/archindexer" \
cargo sqlx prepare

# Clean up proxy process
if ps -p $PROXY_PID > /dev/null; then
    kill $PROXY_PID
fi

# Deploy infrastructure (after image is available)
cd "$ROOT_DIR/deploy/gcp/terraform"
terraform init
terraform apply -auto-approve

echo "Deployment complete!"