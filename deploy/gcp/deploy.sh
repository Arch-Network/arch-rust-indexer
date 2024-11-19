#!/bin/bash
set -e

# Check required environment variables
if [ -z "$PROJECT_ID" ] || [ -z "$DB_USER" ] || [ -z "$DB_PASSWORD" ]; then
    echo "Error: Required environment variables not set"
    echo "Please set: PROJECT_ID, DB_USER, DB_PASSWORD"
    exit 1
fi

# Create terraform.tfvars
cd deploy/gcp/terraform
cat > terraform.tfvars << EOF
project_id  = "$PROJECT_ID"
region      = "us-central1"
db_username = "$DB_USER"
db_password = "$DB_PASSWORD"
EOF

# Deploy infrastructure
terraform init
terraform apply -auto-approve

# Get database instance connection name
DB_INSTANCE=$(terraform output -raw instance_connection_name)

# Start Cloud SQL Proxy for local database access
echo "Starting Cloud SQL Proxy..."
cloud-sql-proxy "$DB_INSTANCE" --port 5433 &
PROXY_PID=$!
sleep 5

# Run migrations first
echo "Running database migrations..."
DATABASE_URL="postgresql://$DB_USER:$DB_PASSWORD@localhost:5433/archindexer" \
sqlx migrate run

# Generate SQLx prepare files
echo "Generating SQLx prepare files..."
DATABASE_URL="postgresql://$DB_USER:$DB_PASSWORD@localhost:5433/archindexer" \
cargo sqlx prepare

# Kill the proxy
kill $PROXY_PID

# Build and push Docker image
cd ../../..
docker build -t gcr.io/$PROJECT_ID/arch-indexer:latest .
docker push gcr.io/$PROJECT_ID/arch-indexer:latest

echo "Deployment complete!"