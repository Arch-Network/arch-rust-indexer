#!/usr/bin/env bash
set -euo pipefail

# Reusable local compose builder helper
# Usage:
#   scripts/compose_build.sh up        # ensure builder, compose up -d (default)
#   scripts/compose_build.sh build     # ensure builder, compose build
#   scripts/compose_build.sh up --no-cache
#   CLEAN_BUILDER=1 scripts/compose_build.sh up   # stop/rm builder after
#   BUILDER=archlocal DRIVER=docker-container scripts/compose_build.sh up

ACTION=${1:-up}
shift || true

# Config
ROOT_DIR="$(cd "$(dirname "$0")"/.. && pwd)"
MICRO_DIR="$ROOT_DIR/arch-indexer-microservices"
BUILDER=${BUILDER:-archlocal}
# DRIVER can be: docker (no BuildKit container) or docker-container (spawns buildkit container)
DRIVER=${DRIVER:-docker-container}

# Prefer reusing a named builder to avoid stray buildx containers
if ! docker buildx inspect "$BUILDER" >/dev/null 2>&1; then
  docker buildx create --name "$BUILDER" --driver "$DRIVER" --use >/dev/null
  # Only bootstrap for container driver
  if [[ "$DRIVER" == "docker-container" ]]; then
    docker buildx inspect "$BUILDER" --bootstrap >/dev/null || true
  fi
else
  docker buildx use "$BUILDER" >/dev/null
fi

# Ensure BuildKit path is used by compose
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1

cd "$MICRO_DIR"
case "$ACTION" in
  build)
    docker compose build "$@"
    ;;
  up)
    docker compose up -d --build "$@"
    ;;
  *)
    echo "Unknown action: $ACTION (expected: up|build)" >&2
    exit 2
    ;;
esac

# Optional cleanup
if [[ "${CLEAN_BUILDER:-}" == "1" ]]; then
  # stop/rm builder if using container driver
  if [[ "$DRIVER" == "docker-container" ]]; then
    docker buildx rm "$BUILDER" || true
  fi
fi

# Optional cache prune
if [[ "${PRUNE_BUILDX:-}" == "1" ]]; then
  docker buildx prune -a -f || true
fi

echo "Done: compose $ACTION with builder '$BUILDER' (driver=$DRIVER)"
