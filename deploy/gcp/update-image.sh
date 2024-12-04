#!/bin/bash
set -e

# Store the root directory of the project
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Check required environment variables
if [ -z "$PROJECT_ID" ]; then
    echo "Error: PROJECT_ID environment variable not set"
    exit 1
fi

# Build and push Docker image
export DOCKER_BUILDKIT=1
echo "Building and pushing Docker image..."
docker build --platform linux/amd64 -t gcr.io/$PROJECT_ID/arch-rust-indexer:latest .
docker push gcr.io/$PROJECT_ID/arch-rust-indexer:latest

# Deploy new image to Cloud Run
echo "Updating Cloud Run service..."
gcloud run services update arch-rust-indexer \
    --image gcr.io/$PROJECT_ID/arch-rust-indexer:latest \
    --region us-central1 \
    --project $PROJECT_ID

echo "Deployment complete!"
