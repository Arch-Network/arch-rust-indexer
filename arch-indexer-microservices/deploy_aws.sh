#!/usr/bin/env bash
set -euo pipefail

# Config (allow env overrides)
ACCOUNT_ID=${ACCOUNT_ID:-590184001652}
REGION=${REGION:-us-east-1}
AWS_PROFILE_FLAG=${AWS_PROFILE:+--profile $AWS_PROFILE}

API_REPO=${API_REPO:-arch-indexer/api-server}
INDEXER_REPO=${INDEXER_REPO:-arch-indexer/indexer}
FRONTEND_REPO=${FRONTEND_REPO:-arch-indexer/frontend}

# Resolve absolute paths to component directories
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
API_DIR="$SCRIPT_DIR/api-server"
INDEXER_DIR="$SCRIPT_DIR/indexer"
FRONTEND_DIR="$SCRIPT_DIR/frontend"

# Validate directories
for d in "$API_DIR" "$INDEXER_DIR" "$FRONTEND_DIR"; do
  if [ ! -d "$d" ]; then
    echo "ERROR: Directory not found: $d" >&2
    exit 1
  fi
done

# Verify AWS auth (non-interactive). If this fails, run: aws sso login [--profile <profile>]
if ! aws sts get-caller-identity $AWS_PROFILE_FLAG >/dev/null 2>&1; then
  echo "ERROR: AWS auth failed. Run: aws sso login ${AWS_PROFILE:+--profile $AWS_PROFILE}" >&2
  exit 1
fi

# Ensure repos exist
aws ecr create-repository --repository-name "$API_REPO" --region "$REGION" $AWS_PROFILE_FLAG || true
aws ecr create-repository --repository-name "$INDEXER_REPO" --region "$REGION" $AWS_PROFILE_FLAG || true
aws ecr create-repository --repository-name "$FRONTEND_REPO" --region "$REGION" $AWS_PROFILE_FLAG || true

# Login to ECR
aws ecr get-login-password --region "$REGION" $AWS_PROFILE_FLAG \
  | docker login --username AWS --password-stdin "$ACCOUNT_ID.dkr.ecr.$REGION.amazonaws.com"

# Tag
TAG=${TAG:-$(git -C "$SCRIPT_DIR" rev-parse --short HEAD)}
ECR_API="$ACCOUNT_ID.dkr.ecr.$REGION.amazonaws.com/$API_REPO"
ECR_INDEXER="$ACCOUNT_ID.dkr.ecr.$REGION.amazonaws.com/$INDEXER_REPO"
ECR_FRONTEND="$ACCOUNT_ID.dkr.ecr.$REGION.amazonaws.com/$FRONTEND_REPO"

build_push() {
  local name="$1" repo="$2" dir="$3" dockerfile="$4"
  echo "\n=== Building $name ($repo:$TAG) for linux/amd64 ==="
  # Ensure buildx is available
  docker buildx use default >/dev/null 2>&1 || docker buildx create --use >/dev/null 2>&1 || true
  docker buildx build --platform linux/amd64 -t "$repo:$TAG" -t "$repo:latest" -f "$dockerfile" "$dir" --push
}

# Build and push all images (linux/amd64)
build_push api-server "$ECR_API" "$API_DIR" "$API_DIR/Dockerfile"
build_push indexer    "$ECR_INDEXER" "$INDEXER_DIR" "$INDEXER_DIR/Dockerfile"
build_push frontend   "$ECR_FRONTEND" "$FRONTEND_DIR" "$FRONTEND_DIR/Dockerfile"

echo "\nDone. Images pushed:"
echo "  $ECR_API:$TAG"
echo "  $ECR_INDEXER:$TAG"
echo "  $ECR_FRONTEND:$TAG"
