#!/bin/bash

set -e

# Source the appropriate .env file
if [ "$ENVIRONMENT" = "production" ]; then
    ENV_FILE=".env.production"
else
    ENV_FILE=".env.development"
fi

if [ ! -f "$ENV_FILE" ]; then
    echo "Error: $ENV_FILE not found"
    exit 1
fi

source "$ENV_FILE"

# Check required variables
required_vars=(
    "DB_USERNAME"
    "DB_PASSWORD"
    "DB_NAME"
    "ARCH_NODE_URL"
)

for var in "${required_vars[@]}"; do
    if [ -z "${!var}" ]; then
        echo "Error: $var is not set in $ENV_FILE"
        exit 1
    fi
done

echo "Environment validation successful"